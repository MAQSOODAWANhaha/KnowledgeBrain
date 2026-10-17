"""Offline deployment contracts; renders Compose only, never contacts Docker daemon."""
import json
import os
from pathlib import Path
import runpy
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SAMPLE = {
    "POSTGRES_USER": "test_app",
    "POSTGRES_PASSWORD": "test database @:/?#% password",
    "KB_ADMIN_USERNAME": "test_admin",
    "KB_ADMIN_PASSWORD": "test-local-password-only",
    "JWT_SECRET": "test-jwt-secret-only-not-real-credential",
    "MINIO_ROOT_USER": "test_storage",
    "MINIO_ROOT_PASSWORD": "test-storage-password-only",
    "KNOWLEDGEBRAIN_CHAT_BASE_URL": "https://model.invalid/v1",
    "KNOWLEDGEBRAIN_CHAT_API_KEY": "test-model-key-only",
    "KNOWLEDGEBRAIN_CHAT_MODEL": "test-model",
    "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS": "131072",
}


def render(extra=None, omit=None, local=False, python_tender=False, runtime=True):
    env = {"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/tmp"), **SAMPLE}
    env.update(extra or {})
    if omit:
        env.pop(omit)
    args = ["docker", "compose", "--env-file", "/dev/null", "-f", "deploy/docker-compose.yml"]
    if local:
        args += ["-f", "deploy/docker-compose.local.yml"]
    if runtime:
        args += ["--profile", "runtime"]
    if python_tender:
        args += ["--profile", "python-tender"]
    args += ["config", "--format", "json"]
    result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    return result


class DeploymentConfiguration(unittest.TestCase):
    def test_minimal_runtime_has_one_account_and_no_optional_services(self):
        result = render()
        self.assertEqual(result.returncode, 0, result.stderr)
        cfg = json.loads(result.stdout)
        services = cfg["services"]
        self.assertNotIn("neo4j", services)
        self.assertNotIn("onlyoffice", services)
        self.assertNotIn("python-tender-worker", services)
        for name in ("migrate", "api", "worker", "retention"):
            env = services[name]["environment"]
            self.assertEqual(env["POSTGRES_USER"], SAMPLE["POSTGRES_USER"])
            self.assertEqual(env["POSTGRES_PASSWORD"], SAMPLE["POSTGRES_PASSWORD"])
            self.assertEqual(env["DATABASE_URL"], "")
            self.assertFalse(any("_DB_PASSWORD" in key for key in env))
        self.assertEqual(services["postgres"]["environment"]["POSTGRES_USER"], "postgres")
        self.assertNotIn("POSTGRES_PASSWORD", services["postgres"]["environment"])
        for name in ("postgres", "redis", "minio", "docreader"):
            self.assertFalse(services[name].get("ports"))
        app = services["api"]["environment"]
        self.assertEqual(app["KNOWLEDGEBRAIN_VLM_API_KEY"], "")
        self.assertEqual(app["KNOWLEDGEBRAIN_S3_SECRET_KEY"], "")
        self.assertEqual(app["KNOWLEDGEBRAIN_KNOWLEDGE_ENABLED"], "false")
        self.assertNotIn("KB_TENDER_AGENT_LIMITS", app)
        for service in services.values():
            env = service.get("environment", {})
            for key in ("KB_DEPLOYMENT_NAMESPACE_ID", "KB_RELEASE_DESCRIPTOR_PATH",
                        "KB_RELEASE_DESCRIPTOR_SHA256", "KB_COMPONENT_KIND",
                        "KB_COMPONENT_IMAGE_DIGEST"):
                self.assertNotIn(key, env)

    def test_required_values_have_no_secret_fallback(self):
        for key in ("POSTGRES_USER", "POSTGRES_PASSWORD", "JWT_SECRET", "MINIO_ROOT_USER",
                    "MINIO_ROOT_PASSWORD", "KNOWLEDGEBRAIN_CHAT_BASE_URL",
                    "KNOWLEDGEBRAIN_CHAT_API_KEY", "KNOWLEDGEBRAIN_CHAT_MODEL",
                    "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS"):
            with self.subTest(key=key):
                result = render(omit=key)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(key, result.stderr)

    def test_local_ports_and_client_follow_overrides(self):
        result = render({"API_PORT": "8181", "DOCREADER_GRPC_PORT": "5151"}, local=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        services = json.loads(result.stdout)["services"]
        self.assertEqual(services["retention"]["image"], "knowledgebrain-worker:local")
        self.assertEqual(services["caddy"]["environment"]["API_UPSTREAM"], "api:8181")
        self.assertNotIn("onlyoffice", services["caddy"]["depends_on"])
        self.assertEqual(services["api"]["environment"]["DOCREADER_ADDR"], "docreader:5151")
        self.assertEqual(services["docreader"]["environment"]["DOCREADER_GRPC_PORT"], "5151")

    def test_python_roles_share_database_redis_models_and_artifacts(self):
        password = " database @:/?#% '$\\ password "
        result = render({"POSTGRES_PASSWORD": password}, python_tender=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        cfg = json.loads(result.stdout)
        self.assertEqual(cfg["name"], "kb")
        services = cfg["services"]
        api = services["api"]["environment"]
        for name in ("python-tender-init", "python-tender-worker", "python-tender-dispatcher"):
            role = services[name]
            env = role["environment"]
            self.assertEqual(env["DATABASE_URL"], "")
            self.assertEqual(env["PGHOST"], api["POSTGRES_HOST"])
            self.assertEqual(env["PGPORT"], api["POSTGRES_PORT"])
            self.assertEqual(env["PGDATABASE"], api["POSTGRES_DB"])
            self.assertEqual(env["PGUSER"], SAMPLE["POSTGRES_USER"])
            self.assertEqual(env["PGPASSWORD"], api["POSTGRES_PASSWORD"])
            self.assertNotIn("TENDER_DATABASE_URL", env)
            self.assertNotIn("TENDER_REDIS_URL", env)
            self.assertFalse(role.get("ports"))
            self.assertEqual(role["volumes"][0]["source"], "python-tender-artifacts")
            self.assertEqual(role["image"], services["docreader"]["image"])
        for name in ("python-tender-worker", "python-tender-dispatcher"):
            self.assertEqual(services[name]["environment"]["REDIS_URL"], api["REDIS_URL"])
            self.assertEqual(services[name]["depends_on"]["python-tender-init"]["condition"],
                             "service_completed_successfully")
        for key, value in api.items():
            if key.startswith(("KNOWLEDGEBRAIN_CHAT_", "KNOWLEDGEBRAIN_VLM_")) and key != "KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT":
                self.assertEqual(services["python-tender-worker"]["environment"][key], value)

    def test_python_only_startup_waits_for_shared_database_baseline(self):
        result = render(python_tender=True, runtime=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        services = json.loads(result.stdout)["services"]
        self.assertIn("migrate", list(services))
        self.assertNotIn("api", services)
        self.assertNotIn("worker", services)
        self.assertEqual(services["python-tender-init"]["depends_on"]["migrate"]["condition"],
                         "service_completed_successfully")

    def test_role_environments_exclude_unneeded_secrets_and_retired_keys(self):
        result = render(python_tender=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        services = json.loads(result.stdout)["services"]
        for name, service in services.items():
            env = service.get("environment", {})
            for key in ("LLM_BASE_URL", "LLM_API_KEY", "LLM_MODEL", "EMBEDDING_BASE_URL",
                        "EMBEDDING_API_KEY", "EMBEDDING_MODEL", "KNOWLEDGEBRAIN_SHARED_CONCURRENCY",
                        "KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES"):
                self.assertNotIn(key, env)
            if name not in ("api", "onlyoffice"):
                for key in ("JWT_SECRET", "KB_ADMIN_PASSWORD", "KB_ONLYOFFICE_CAPABILITY_SECRET"):
                    self.assertNotIn(key, env)
        for name in ("retention", "python-tender-init", "python-tender-dispatcher"):
            self.assertNotIn("KNOWLEDGEBRAIN_CHAT_API_KEY", services[name]["environment"])
            self.assertNotIn("KNOWLEDGEBRAIN_VLM_API_KEY", services[name]["environment"])
        self.assertNotIn("REDIS_URL", services["python-tender-init"]["environment"])


class PythonWorkflowSettings(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = runpy.run_path(str(ROOT / "services/docreader/jobs/settings.py"))

    def test_shared_libpq_settings_need_no_second_database_url(self):
        env = {"PGHOST": "postgres", "PGUSER": "test_app", "PGPASSWORD": " '$\\:/?#% secret ",
               "PGDATABASE": "knowledgebrain", "REDIS_URL": "redis://redis:6379"}
        with patch.dict(os.environ, env, clear=True):
            settings = self.module["Settings"].from_env()
            self.assertEqual(settings.dsn, "")
            self.assertEqual(settings.redis_url, env["REDIS_URL"])
            self.assertEqual(os.environ["PGPASSWORD"], env["PGPASSWORD"])

    def test_database_url_and_explicit_operator_overrides(self):
        with patch.dict(os.environ, {"DATABASE_URL": "postgresql://shared.invalid/db",
                                     "REDIS_URL": "redis://shared.invalid/0"}, clear=True):
            self.assertEqual(self.module["Settings"].from_env().dsn, os.environ["DATABASE_URL"])
            os.environ["TENDER_DATABASE_URL"] = "postgresql://isolated.invalid/test_db"
            os.environ["TENDER_REDIS_URL"] = "redis://isolated.invalid/1"
            settings = self.module["Settings"].from_env()
            self.assertEqual(settings.dsn, os.environ["TENDER_DATABASE_URL"])
            self.assertEqual(settings.redis_url, os.environ["TENDER_REDIS_URL"])

    def test_missing_database_configuration_does_not_use_an_implicit_database(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, "DATABASE_URL or PostgreSQL PGHOST"):
                self.module["Settings"].from_env()



if __name__ == "__main__":
    unittest.main()
