"""Offline request recovery and real-identity handoff checks; never call a model."""
import copy
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).parents[1]))
import bidding_full_flow_acceptance as flow
from onlyoffice_product_probe import verify_attached_identity


@pytest.mark.parametrize("suffix,media_type", [
    (".PDF", "application/pdf"),
    (".docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
])
def test_upload_preserves_source_bytes_and_uses_actual_media_type_without_model(tmp_path, suffix, media_type):
    original = b"%PDF-1.7\r\n\x00\xff source byte identity fixture\r\n"
    source = tmp_path / ("original" + suffix)
    source.write_bytes(original)

    class ParsingBoundary(Exception):
        pass

    class Api:
        calls = []

        def request(self, method, path, body, headers, expected):
            self.calls.append((method, path, body, headers, expected))
            if path == "/api/v2/bid-projects":
                return flow.json_bytes(dict(id="project", workspace_id="workspace"))
            assert path == "/api/v2/bid-projects/project/tender-documents", "no analysis/model request permitted"
            boundary = headers["Content-Type"].split("boundary=", 1)[1].encode()
            prefix, payload = body.split(b"\r\n\r\n", 1)
            assert f"Content-Type: {media_type}".encode() in prefix
            assert f'filename="original{suffix}"'.encode() in prefix
            assert payload == original + b"\r\n--" + boundary + b"--\r\n"
            return flow.json_bytes(dict(id="document", original_sha256=flow.digest(original)))

        def get(self, path):
            assert path == "/api/v2/bid-projects/project/tender-documents"
            raise ParsingBoundary("stop before freeze or model work")

    api = Api()
    driver = flow.Driver(tmp_path / "run", {"source_sha256": flow.digest(original)}, api)
    with pytest.raises(ParsingBoundary):
        flow.run(driver, source, {}, 1, 0.01)
    assert len(api.calls) == 2
    assert (driver.root / "upload.request").read_bytes() == api.calls[1][2]
    assert source.read_bytes() == original


def test_unknown_source_suffix_rejected_before_api_mutation(tmp_path):
    class Api:
        def request(self, *_args):
            raise AssertionError("unsupported source must not reach API")

    driver = flow.Driver(tmp_path / "run", {}, Api())
    with pytest.raises(ValueError, match="PDF or DOCX"):
        flow.run(driver, tmp_path / "original.txt", {}, 1, 0.01)
    assert not driver.state["mutations"]


def test_connection_requires_current_env_and_matching_startup_snapshot(tmp_path):
    env = tmp_path / ".env"
    env.write_text("LLM_MODEL=from-file\nLLM_API_KEY=private-fixture\nLLM_BASE_URL=https://configured.invalid/v1\n"
                   "KB_AUTHORING_MAX_OUTPUT_TOKENS=4096\nKB_AUTHORING_TIMEOUT_MS=90000\n")
    config = {"provider": {"model_id": "from-file", "base_url": "https://configured.invalid/v1",
                          "protocol": "openai_chat_completions_sse", "max_tokens": 4096, "timeout_ms": 90000},
              "limits": {"max_turns": 10}}
    value = {"origin": "http://127.0.0.1:1234", "token": "private-auth", "startup": {
        "env_file_sha256": flow.digest(env.read_bytes()), "runtime": {"analysis": config}}}
    ticket = tmp_path / "connection.json"
    flow.private_write(ticket, flow.json_bytes(value))
    assert flow.read_connection(ticket, env) == value
    value["startup"]["runtime"]["analysis"]["provider"]["model_id"] = "temporary-override"
    flow.private_write(ticket, flow.json_bytes(value))
    with pytest.raises(ValueError, match="snapshot differs"):
        flow.read_connection(ticket, env)
    env.write_text(env.read_text() + "# updated after service startup\n")
    with pytest.raises(ValueError, match="startup snapshot"):
        flow.read_connection(ticket, env)
    ticket.chmod(0o644)
    with pytest.raises(ValueError, match="private"):
        flow.read_connection(ticket, env)


def test_lost_mutation_receipt_reuses_bytes_key_and_does_not_rebuild_intent(tmp_path):
    receipt = {"request_artifact_id": "request", "request_revision": 1, "frozen_input_sha256": "a" * 64}
    class Api:
        calls = []
        def request(self, *args):
            self.calls.append(args)
            if len(self.calls) == 1:
                raise OSError("lost acknowledgment")
            return flow.json_bytes(receipt)
    api = Api()
    driver = flow.Driver(tmp_path, {"source": "same"}, api)
    with pytest.raises(OSError):
        driver.post("analysis", "/freeze", lambda: {"original": True}, 201)
    resumed = flow.Driver(tmp_path, {"source": "same"}, api)
    def must_not_rebuild():
        raise AssertionError("must use original body, even if current source changed")
    assert resumed.post("analysis", "/freeze", must_not_rebuild, 201) == receipt
    assert api.calls[0] == api.calls[1]
    assert resumed.post("analysis", "/freeze", must_not_rebuild, 201) == receipt
    assert len(api.calls) == 2
    with pytest.raises(ValueError, match="persisted mutation"):
        resumed.post("analysis", "/new-freeze", must_not_rebuild, 201)
    with pytest.raises(ValueError, match="another source"):
        flow.Driver(tmp_path, {"source": "different"}, api)


def test_failed_or_changed_job_cannot_be_promoted_or_recreated(tmp_path):
    receipt = {"request_artifact_id": "request", "request_revision": 1, "frozen_input_sha256": "a" * 64}
    class Api:
        value = dict(receipt, status="failed", error_code="REVIEW_REJECTED")
        def get(self, _path):
            return self.value
        def request(self, *_args):
            raise AssertionError("observing failure must not create or continue a request")
    api = Api()
    driver = flow.Driver(tmp_path, {}, api)
    with pytest.raises(RuntimeError, match="original request retained"):
        driver.job("analysis", "/status", receipt, 1, 0.01)
    assert driver.state["observations"]["analysis"]["request_artifact_id"] == "request"
    assert not driver.state["mutations"]
    api.value = dict(receipt, status="succeeded", request_artifact_id="replacement")
    with pytest.raises(ValueError, match="identity changed"):
        driver.job("analysis", "/status", receipt, 1, 0.01)


def test_malformed_receipt_retains_uncertain_attempt(tmp_path):
    class Api:
        def request(self, *_args):
            return b'{"request_artifact_id":"incomplete"}'
    driver = flow.Driver(tmp_path, {}, Api())
    with pytest.raises(ValueError, match="original intent retained"):
        driver.post("analysis", "/freeze", lambda: {}, 201)
    assert driver.state["mutations"]["analysis"]["receipt"] is None
    assert driver.state["mutations"]["analysis"]["key"]


def test_handoff_matches_existing_office_probe_and_rejects_newer_saved_version():
    source = b"offline saved DOCX identity fixture"
    sha = flow.digest(source)
    basis = dict(document_set_id="documents", document_set_sha256="a" * 64,
                 requirement_set_id="requirements", requirement_set_sha256="b" * 64)
    current = dict(version_id="version", round_id="round", docx_sha256=sha, project_id="project", workspace_id="workspace",
                   round_basis=basis, editor=dict(pending_save_id=None, save_error=None))
    project = dict(id="project", workspace_id="workspace")
    connection = dict(origin="http://127.0.0.1:1234", token="private")
    ticket = flow.handoff(connection, project, current, basis, current, source)
    assert verify_attached_identity(ticket, current, source, lambda path: source) == current
    changed = copy.deepcopy(current)
    changed["version_id"] = "newer-version"
    with pytest.raises(ValueError, match="changed after outline"):
        flow.handoff(connection, project, changed, basis, current, source)
    with pytest.raises(ValueError, match="digest mismatch"):
        flow.handoff(connection, project, current, basis, current, source + b"edit")
    changed = copy.deepcopy(current)
    changed["round_basis"]["requirement_set_id"] = "fixture-instead-of-analysis"
    with pytest.raises(ValueError, match="another analysis basis"):
        flow.handoff(connection, project, changed, basis, current, source)


def test_outline_publication_handoff_does_not_start_another_composer(tmp_path):
    source = tmp_path / "tender.pdf"
    source.write_bytes(b"original tender")
    docx = b"generated outline"
    basis = dict(document_set_id="set", document_set_sha256="a" * 64,
                 requirement_set_id="requirements", requirement_set_sha256="b" * 64)
    generated = dict(version_id="version", round_id="round", docx_sha256=flow.digest(docx))
    current = dict(generated, project_id="project", workspace_id="workspace", round_basis=basis,
                   editor=dict(pending_save_id=None, save_error=None))

    class Api:
        def get(self, path):
            if path.endswith("/docx-fills/basis"):
                return basis
            if path.endswith("/docx/current"):
                return current
            raise AssertionError(path)

        def request(self, method, path):
            assert method == "GET" and path.endswith("/docx/versions/version/download")
            return docx

    class Driver:
        api = Api()
        root = tmp_path
        state = {"identity": {"source_sha256": flow.digest(source.read_bytes())}}

        def post(self, name, path, prepare, status):
            if name == "project":
                return dict(id="project", workspace_id="workspace")
            assert name == "analysis", "outline generation is the only model job"
            return dict(request_artifact_id="request")

        def mutation(self, *args):
            return dict(id="document", original_sha256=self.state["identity"]["source_sha256"])

        def observe(self, *args):
            pass

        def job(self, name, *args):
            assert name == "analysis"
            return dict(document_set_revision_id="set", document_set_sha256="a" * 64,
                        result_identity=dict(requirement_set_id="requirements",
                                             requirement_set_sha256="b" * 64, draft_docx=generated))

        def save(self):
            pass

    driver = Driver()
    flow.run(driver, source, dict(origin="http://127.0.0.1:1234", token="private"), 1, 0.01)
    assert driver.state["stage"] == "outline_published"
    assert (tmp_path / "generated.docx").read_bytes() == docx
