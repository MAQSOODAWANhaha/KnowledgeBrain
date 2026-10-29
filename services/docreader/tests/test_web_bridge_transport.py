"""The host command bridge uses the existing authenticated gRPC boundary."""
import json
from concurrent import futures

import grpc
import pytest

from docreader.auth import AuthInterceptor
from docreader.proto import docreader_pb2, docreader_pb2_grpc


def test_tender_bridge_authenticates_and_returns_the_real_handler_result(monkeypatch):
    from docreader.integrations import web
    from docreader.main import TenderBridgeServicer

    monkeypatch.setenv("GRPC_AUTH_TOKEN", "isolated-tender-transport-token")
    calls = []

    async def handle(payload, blob):
        calls.append((payload, blob))
        return {"status": "saved", "size": len(blob)}, blob

    monkeypatch.setattr(web, "handle_command", handle)
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2), interceptors=[AuthInterceptor()])
    docreader_pb2_grpc.add_TenderBridgeServicer_to_server(TenderBridgeServicer(), server)
    port = server.add_insecure_port("127.0.0.1:0")
    server.start()
    try:
        with grpc.insecure_channel(f"127.0.0.1:{port}") as channel:
            client = docreader_pb2_grpc.TenderBridgeStub(channel)
            request = docreader_pb2.TenderCommandRequest(json_payload=b'{"action":"document.put"}', blob=b"original")
            with pytest.raises(grpc.RpcError) as rejected:
                client.Command(request, timeout=5)
            assert rejected.value.code() == grpc.StatusCode.UNAUTHENTICATED
            assert not calls
            reply = client.Command(request, metadata=[("authorization", "Bearer isolated-tender-transport-token")], timeout=5)
            assert json.loads(reply.json_payload) == {"status": "saved", "size": 8}
            assert reply.blob == b"original"
            assert calls == [({"action": "document.put"}, b"original")]
    finally:
        server.stop(0).wait(5)


def test_command_bridge_requires_explicit_service_authentication(monkeypatch):
    from docreader.main import TenderBridgeServicer

    monkeypatch.delenv("GRPC_AUTH_TOKEN", raising=False)

    class Aborted(RuntimeError):
        pass

    class Context:
        def abort(self, code, detail):
            assert code == grpc.StatusCode.FAILED_PRECONDITION
            raise Aborted(detail)

    with pytest.raises(Aborted, match="GRPC_AUTH_TOKEN"):
        TenderBridgeServicer().Command(docreader_pb2.TenderCommandRequest(json_payload=b"{}"), Context())


def test_command_bridge_preserves_conflict_status_without_exposing_internal_detail(monkeypatch):
    from docreader.integrations import web
    from docreader.integrations.store import Conflict
    from docreader.main import TenderBridgeServicer

    monkeypatch.setenv("GRPC_AUTH_TOKEN", "isolated-tender-transport-token")
    async def conflict(payload, blob):
        raise Conflict("private request digest and source details")
    monkeypatch.setattr(web, "handle_command", conflict)
    reply = TenderBridgeServicer().Command(docreader_pb2.TenderCommandRequest(json_payload=b'{}'), object())
    error = json.loads(reply.json_payload)["error"]
    assert error["status"] == 409 and error["code"] == "REQUEST_CONFLICT"
    assert "private" not in error["message"] and reply.blob == b""
