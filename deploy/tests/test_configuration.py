"""Offline deployment contracts; renders Compose only, never contacts Docker daemon."""
import json
import os
from pathlib import Path
import subprocess
import unittest

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


def render(extra=None, omit=None, local=False):
    env = {"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/tmp"), **SAMPLE}
    env.update(extra or {})
    if omit:
        env.pop(omit)
    args = ["docker", "compose", "--env-file", "/dev/null", "-f", "deploy/docker-compose.yml"]
    if local:
        args += ["-f", "deploy/docker-compose.local.yml"]
    args += ["--profile", "runtime", "config", "--format", "json"]
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



if __name__ == "__main__":
    unittest.main()
