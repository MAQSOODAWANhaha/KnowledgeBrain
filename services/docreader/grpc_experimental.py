"""Typed re-export of grpc.experimental simple stubs.

typeshed's ``grpc`` stub has no ``experimental`` attribute. Importing the
submodule via importlib keeps runtime behavior and gives checkers a real type.
"""

from __future__ import annotations

import importlib
from collections.abc import Callable, Iterator
from typing import Any

_exp = importlib.import_module("grpc.experimental")

unary_unary: Callable[..., Any] = _exp.unary_unary
unary_stream: Callable[..., Iterator[Any]] = _exp.unary_stream
