from collections.abc import Callable, Iterator, Sequence
from typing import Any

import grpc

def unary_unary(
    request: object,
    target: str,
    method: str,
    request_serializer: Callable[[Any], bytes] | None = None,
    response_deserializer: Callable[[bytes], Any] | None = None,
    options: Sequence[tuple[Any, Any]] = (),
    channel_credentials: grpc.ChannelCredentials | None = None,
    insecure: bool = False,
    call_credentials: grpc.CallCredentials | None = None,
    compression: grpc.Compression | None = None,
    wait_for_ready: bool | None = None,
    timeout: float | None = None,
    metadata: Sequence[tuple[str, str | bytes]] | None = None,
    _registered_method: bool | None = False,
) -> Any: ...
def unary_stream(
    request: object,
    target: str,
    method: str,
    request_serializer: Callable[[Any], bytes] | None = None,
    response_deserializer: Callable[[bytes], Any] | None = None,
    options: Sequence[tuple[Any, Any]] = (),
    channel_credentials: grpc.ChannelCredentials | None = None,
    insecure: bool = False,
    call_credentials: grpc.CallCredentials | None = None,
    compression: grpc.Compression | None = None,
    wait_for_ready: bool | None = None,
    timeout: float | None = None,
    metadata: Sequence[tuple[str, str | bytes]] | None = None,
    _registered_method: bool | None = False,
) -> Iterator[Any]: ...
