#!/bin/bash
set -ex

# Run from services/docreader (Makefile `proto` target).
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROTO_DIR="${ROOT}/proto"
PYTHON_OUT="${ROOT}/proto"

uv run python -m grpc_tools.protoc -I${PROTO_DIR} \
    --python_out=${PYTHON_OUT} \
    --pyi_out=${PYTHON_OUT} \
    --grpc_python_out=${PYTHON_OUT} \
    ${PROTO_DIR}/docreader.proto

# 修复Python导入问题（MacOS兼容版本）
if [ "$(uname)" == "Darwin" ]; then
    # MacOS版本
    sed -i '' 's/^import docreader_pb2/from docreader.proto import docreader_pb2/' ${PYTHON_OUT}/docreader_pb2_grpc.py
else
    # Linux版本
    sed -i 's/^import docreader_pb2/from docreader.proto import docreader_pb2/' ${PYTHON_OUT}/docreader_pb2_grpc.py
fi

# typeshed's grpc stub has no `experimental` attribute. Route generated
# simple-stub calls through the typed re-export in grpc_experimental.
GRPC_PY="${PYTHON_OUT}/docreader_pb2_grpc.py" uv run python - <<'PY'
import os
from pathlib import Path

path = Path(os.environ["GRPC_PY"])
text = path.read_text()
old_import = "import grpc\nimport warnings\n"
new_import = (
    "import grpc\n"
    "import warnings\n"
    "from docreader.grpc_experimental import unary_stream, unary_unary\n"
)
if "from docreader.grpc_experimental import" not in text:
    if old_import in text:
        text = text.replace(old_import, new_import, 1)
    else:
        text = text.replace(
            "from grpc.experimental import unary_stream, unary_unary\n",
            "from docreader.grpc_experimental import unary_stream, unary_unary\n",
            1,
        )
text = text.replace("grpc.experimental.unary_unary", "unary_unary")
text = text.replace("grpc.experimental.unary_stream", "unary_stream")
path.write_text(text)
PY

echo "Proto files generated successfully!"