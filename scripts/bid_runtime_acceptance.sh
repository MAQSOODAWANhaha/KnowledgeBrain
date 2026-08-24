#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:?BASE_URL is required, for example http://127.0.0.1:18081}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE_DIR="${EVIDENCE_DIR:-$ROOT/web/e2e/artifacts/runtime}"
ACCEPTANCE_RUNTIME_MODE="${ACCEPTANCE_RUNTIME_MODE:-external-runtime-not-classified}"
ACCEPTANCE_EXTRACT_MODE="${ACCEPTANCE_EXTRACT_MODE:-${BID_EXTRACT_MODE:-not-recorded}}"
ACCEPTANCE_INPUT_MODE="${ACCEPTANCE_INPUT_MODE:-not-recorded}"
ACCEPTANCE_DOCREADER_MODE="${ACCEPTANCE_DOCREADER_MODE:-not-verified}"
ACCEPTANCE_BEFORE_END_HOOK="${ACCEPTANCE_BEFORE_END_HOOK:-}"
TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT
mkdir -p "$EVIDENCE_DIR"

fail() {
  echo "bidding runtime acceptance failed: $*" >&2
  for file in "$TMP"/*.json "$TMP"/*.txt; do
    [ -f "$file" ] || continue
    echo "--- $file" >&2
    tail -n 100 "$file" >&2 || true
  done
  exit 1
}

new_id() {
  if [ -r /proc/sys/kernel/random/uuid ]; then
    tr '[:upper:]' '[:lower:]' </proc/sys/kernel/random/uuid
  else
    python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
  fi
}

get_json() {
  local path=$1 output=$2
  curl -fsS "$BASE_URL$path" -H "$AUTH" -o "$output" || fail "GET $path"
}

json_request() {
  local method=$1 path=$2 body=$3 output=$4 status
  status="$(curl -sS -X "$method" "$BASE_URL$path" \
    -H "$AUTH" -H 'content-type: application/json' \
    -H "Idempotency-Key: $(new_id)" \
    --data "$body" -o "$output" -w '%{http_code}')" || fail "$method $path"
  case "$status" in 200 | 201 | 202 | 204) ;; *) fail "$method $path returned HTTP $status" ;; esac
}

download_output() {
  local project_id=$1 output_id=$2 destination=$3 headers=$4
  curl -fsS "$BASE_URL/api/v1/bids/$project_id/submission/artifacts/$output_id" \
    -H "$AUTH" -D "$headers" -o "$destination" || fail "download output $output_id"
  [ -s "$destination" ] || fail "downloaded output $output_id is empty"
}

wait_for_render_job() {
  local project_id=$1 manifest_id=$2 render_job_id=$3 format=$4 output=$5 output_id status
  for _ in $(seq 1 180); do
    get_json "/api/v1/bids/$project_id/submission/render-jobs/$render_job_id" "$output"
    jq -e --arg render_job_id "$render_job_id" --arg manifest_id "$manifest_id" '
      .render_job_id == $render_job_id
      and .manifest_id == $manifest_id
      and (.status == "pending" or .status == "running" or .status == "completed" or .status == "failed")
    ' "$output" >/dev/null || fail "render job identity/status response is invalid"
    status="$(jq -r .status "$output")"
    [ "$status" != "failed" ] ||
      fail "$format render failed: $(jq -r '.error_code // "SUBMISSION_RENDER_FAILED"' "$output")"
    if [ "$status" = "completed" ]; then
      output_id="$(jq -er '.output_id | select(type == "string" and length > 0)' "$output")" ||
        fail "completed $format render job has no output_id"
      printf '%s\n' "$output_id"
      return 0
    fi
    sleep 1
  done
  fail "$format render output for manifest $manifest_id did not publish within 180 seconds"
}

wait_for_knowledge_document() {
  local document_id=$1 output=$2 status index_ready chunk_count
  for _ in $(seq 1 180); do
    get_json "/api/v1/documents/$document_id/content" "$output"
    status="$(jq -r '.parse_status | if type == "string" then . else .status // "unknown" end' "$output")"
    index_ready="$(jq -r '.index_ready' "$output")"
    chunk_count="$(jq '.chunks | length' "$output")"
    [ "$status" != "failed" ] || fail "knowledge document $document_id processing failed"
    if [ "$status" = "completed" ] && [ "$index_ready" = "true" ] && [ "$chunk_count" -gt 0 ]; then
      return 0
    fi
    sleep 1
  done
  fail "knowledge document $document_id did not publish chunks/index within 180 seconds"
}

assert_error() {
  local actual_status=$1 expected_status=$2 expected_code=$3 output=$4 label=$5
  [ "$actual_status" = "$expected_status" ] ||
    fail "$label returned HTTP $actual_status instead of $expected_status"
  jq -e --arg code "$expected_code" '.error.code == $code' "$output" >/dev/null ||
    fail "$label did not return $expected_code"
}

python3 - "$TMP/tender.docx" "$TMP/tender.pdf" "$TMP/authorization-support.pdf" \
  "$TMP/shot.png" "$ROOT/crates/bid/assets/fonts/NotoSansJP-Regular.otf" <<'PY'
import binascii
import html
import struct
import sys
import zipfile
import zlib

docx_path, pdf_path, attachment_pdf_path, shot_path, font_path = sys.argv[1:]
pdf_lines = [
    "招投标 V1 运行验收文件",
    "技术要求",
    "系统必须具备双千兆网络接口能力，并提供连续原文证据。",
]
docx_lines = [
    "招投标 V1 运行验收补充文件",
    "商务资格",
    "投标人须提供有效的质量管理体系认证证书。",
    "程序要求",
    "投标人须提交授权委托书复印件。",
    "投标函须由授权代表签字并加盖公章。",
    "评分标准",
    "评分标准要求技术响应得分权重为30%。",
    "项目预算为1000.00元。",
]

content_types = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"""
relationships = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"""
paragraphs = "".join(
    f'<w:p><w:r><w:t xml:space="preserve">{html.escape(line)}</w:t></w:r></w:p>'
    for line in docx_lines
)
document_xml = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
    f'<w:body>{paragraphs}<w:sectPr/></w:body></w:document>'
)
with zipfile.ZipFile(docx_path, "w", zipfile.ZIP_DEFLATED) as archive:
    archive.writestr("[Content_Types].xml", content_types)
    archive.writestr("_rels/.rels", relationships)
    archive.writestr("word/document.xml", document_xml)

characters = sorted({ord(char) for line in pdf_lines for char in line})
bfchar_blocks = []
for start in range(0, len(characters), 100):
    block = characters[start : start + 100]
    mappings = "\n".join(f"<{code:04X}> <{code:04X}>" for code in block)
    bfchar_blocks.append(f"{len(block)} beginbfchar\n{mappings}\nendbfchar")
to_unicode = (
    "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
    "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
    "/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n"
    "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
    + "\n".join(bfchar_blocks)
    + "\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n"
).encode("ascii")
content_lines = ["BT", "/F1 11 Tf", "54 790 Td", "15 TL"]
for index, line in enumerate(pdf_lines):
    if index:
        content_lines.append("T*")
    content_lines.append(f"<{line.encode('utf-16-be').hex().upper()}> Tj")
content_lines.append("ET")
content = ("\n".join(content_lines) + "\n").encode("ascii")
with open(font_path, "rb") as source:
    font = source.read()
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /ProcSet [/PDF /Text] /Font << /F1 4 0 R >> >> /Contents 9 0 R >>",
    b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSansJP-Regular /Encoding /Identity-H /DescendantFonts [5 0 R] /ToUnicode 7 0 R >>",
    b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /NotoSansJP-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 6 0 R /DW 1000 /CIDToGIDMap /Identity >>",
    b"<< /Type /FontDescriptor /FontName /NotoSansJP-Regular /Flags 4 /FontBBox [-1000 -1000 2000 2000] /ItalicAngle 0 /Ascent 1160 /Descent -288 /CapHeight 733 /StemV 80 /FontFile3 8 0 R >>",
    f"<< /Length {len(to_unicode)} >>\nstream\n".encode("ascii") + to_unicode + b"endstream",
    f"<< /Length {len(font)} /Subtype /OpenType >>\nstream\n".encode("ascii") + font + b"\nendstream",
    f"<< /Length {len(content)} >>\nstream\n".encode("ascii") + content + b"endstream",
]
pdf = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
offsets = [0]
for number, obj in enumerate(objects, 1):
    offsets.append(len(pdf))
    pdf.extend(f"{number} 0 obj\n".encode("ascii"))
    pdf.extend(obj)
    pdf.extend(b"\nendobj\n")
xref = len(pdf)
pdf.extend(f"xref\n0 {len(objects) + 1}\n".encode("ascii"))
pdf.extend(b"0000000000 65535 f \n")
for offset in offsets[1:]:
    pdf.extend(f"{offset:010d} 00000 n \n".encode("ascii"))
pdf.extend(
    f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode("ascii")
)
with open(pdf_path, "wb") as output:
    output.write(pdf)

# Keep the procedural attachment content-addressed independently from the
# tender input while preserving a valid, deterministic PDF structure.
attachment_pdf = bytearray(pdf)
attachment_pdf[7] = ord("6")
with open(attachment_pdf_path, "wb") as output:
    output.write(attachment_pdf)

def png_chunk(kind, data):
    checksum = binascii.crc32(kind + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", checksum)

pixels = b"\x00\x1f\x65\xd6\xff\xff\xff\xff\xff" * 2
png = (
    b"\x89PNG\r\n\x1a\n"
    + png_chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 6, 0, 0, 0))
    + png_chunk(b"IDAT", zlib.compress(pixels))
    + png_chunk(b"IEND", b"")
)
with open(shot_path, "wb") as output:
    output.write(png)
PY

[ "$(head -c 5 "$TMP/tender.pdf")" = "%PDF-" ] || fail "generated tender PDF is invalid"
[ "$(head -c 5 "$TMP/authorization-support.pdf")" = "%PDF-" ] ||
  fail "generated authorization-support PDF is invalid"
[ "$(od -An -tx1 -N8 "$TMP/shot.png" | tr -d ' \n')" = "89504e470d0a1a0a" ] ||
  fail "generated shot PNG is invalid"
python3 - "$TMP/tender.docx" <<'PY' || fail "generated tender DOCX is invalid"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    assert "评分标准要求技术响应得分权重为30%。" in archive.read("word/document.xml").decode("utf-8")
PY

TOKEN="$(curl -fsS -X POST "$BASE_URL/api/v1/auth/login" \
  -H 'content-type: application/json' \
  --data '{"email":"bid-runtime-acceptance@local","password":"ignored"}' | jq -er .token)" ||
  fail "login"
AUTH="Authorization: Bearer $TOKEN"
ENDS_AT="$(date -u -d '+30 days' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -v+30d +%Y-%m-%dT%H:%M:%SZ)"
SUBMISSION_DATE="$(TZ=Asia/Shanghai date +%F)"

json_request POST /api/v1/bids \
  "{\"title\":\"招投标 V1 运行验收\",\"ends_at\":\"$ENDS_AT\"}" \
  "$TMP/project.json"
PROJECT_ID="$(jq -er .id "$TMP/project.json")" || fail "project id missing"

# Build both live retrieval branches before matching. The disposable product
# document is deleted later; retained documents remain for the subsequent live UI run.
SLUG_SUFFIX="$(printf '%s' "$PROJECT_ID" | cut -c1-8)"
json_request POST /api/v1/workspaces \
  "{\"name\":\"验收产品线\",\"slug\":\"acceptance-product-line-$SLUG_SUFFIX\",\"kind\":\"product_line\"}" \
  "$TMP/product-workspace.json"
PRODUCT_WORKSPACE_ID="$(jq -er .id "$TMP/product-workspace.json")" || fail "product-line workspace id missing"
json_request POST "/api/v1/workspaces/$PRODUCT_WORKSPACE_ID/products" \
  "{\"name\":\"双千兆安全网关\",\"slug\":\"acceptance-gateway-$SLUG_SUFFIX\",\"kind\":\"product\"}" \
  "$TMP/product.json"
PRODUCT_ID="$(jq -er .id "$TMP/product.json")" || fail "product id missing"
json_request POST "/api/v1/products/$PRODUCT_ID/versions" \
  '{"label":"runtime-acceptance","make_current":true}' "$TMP/product-version.json"
PRODUCT_VERSION_ID="$(jq -er 'select(.status=="active" and .current==true) | .id' "$TMP/product-version.json")" ||
  fail "product version is not active/current"
json_request POST "/api/v1/workspaces/$PRODUCT_WORKSPACE_ID/products" \
  "{\"name\":\"双千兆安全网关增强版\",\"slug\":\"acceptance-gateway-alt-$SLUG_SUFFIX\",\"kind\":\"product\"}" \
  "$TMP/alternate-product.json"
ALTERNATE_PRODUCT_ID="$(jq -er .id "$TMP/alternate-product.json")" || fail "alternate product id missing"
json_request POST "/api/v1/products/$ALTERNATE_PRODUCT_ID/versions" \
  '{"label":"runtime-acceptance","make_current":true}' "$TMP/alternate-product-version.json"
ALTERNATE_PRODUCT_VERSION_ID="$(jq -er 'select(.status=="active" and .current==true) | .id' "$TMP/alternate-product-version.json")" ||
  fail "alternate product version is not active/current"

json_request POST /api/v1/workspaces \
  "{\"name\":\"验收公司资料\",\"slug\":\"acceptance-company-$SLUG_SUFFIX\",\"kind\":\"company\"}" \
  "$TMP/company-workspace.json"
COMPANY_WORKSPACE_ID="$(jq -er .id "$TMP/company-workspace.json")" || fail "company workspace id missing"
json_request POST "/api/v1/workspaces/$COMPANY_WORKSPACE_ID/products" \
  "{\"name\":\"公司资格材料\",\"slug\":\"acceptance-company-library-$SLUG_SUFFIX\",\"kind\":\"library\"}" \
  "$TMP/company-product.json"
COMPANY_PRODUCT_ID="$(jq -er .id "$TMP/company-product.json")" || fail "company product id missing"
json_request POST "/api/v1/products/$COMPANY_PRODUCT_ID/versions" \
  '{"label":"runtime-acceptance","make_current":true}' "$TMP/company-version.json"
COMPANY_VERSION_ID="$(jq -er 'select(.status=="active" and .current==true) | .id' "$TMP/company-version.json")" ||
  fail "company version is not active/current"

json_request POST "/api/v1/products/$PRODUCT_ID/versions/$PRODUCT_VERSION_ID/documents/manual" \
  '{"title":"双千兆网关技术白皮书","content":"双千兆安全网关技术白皮书明确：系统必须具备双千兆网络接口能力，并提供连续原文证据。设备接口速率为1000Mbps。"}' \
  "$TMP/product-retained-document.json"
PRODUCT_RETAINED_DOCUMENT_ID="$(jq -er .id "$TMP/product-retained-document.json")" ||
  fail "retained product knowledge document id missing"
json_request POST "/api/v1/products/$PRODUCT_ID/versions/$PRODUCT_VERSION_ID/documents/manual" \
  '{"title":"双千兆网关补充规格","content":"补充规格确认：系统必须具备双千兆网络接口能力，并提供连续原文证据。接口速率为1000Mbps并可连续运行。"}' \
  "$TMP/product-disposable-document.json"
PRODUCT_DELETED_DOCUMENT_ID="$(jq -er .id "$TMP/product-disposable-document.json")" ||
  fail "disposable product knowledge document id missing"
json_request POST "/api/v1/products/$ALTERNATE_PRODUCT_ID/versions/$ALTERNATE_PRODUCT_VERSION_ID/documents/manual" \
  '{"title":"双千兆网关增强版技术白皮书","content":"双千兆安全网关增强版明确支持同一技术要求：系统必须具备双千兆网络接口能力，并提供连续原文证据。设备接口速率为1000Mbps。"}' \
  "$TMP/alternate-product-retained-document.json"
ALTERNATE_PRODUCT_RETAINED_DOCUMENT_ID="$(jq -er .id "$TMP/alternate-product-retained-document.json")" ||
  fail "alternate retained product knowledge document id missing"
json_request POST "/api/v1/products/$COMPANY_PRODUCT_ID/versions/$COMPANY_VERSION_ID/documents/manual" \
  '{"title":"质量管理体系认证","content":"示例网络安全有限公司持有有效的ISO 9001质量管理体系认证证书，证书在投标有效期内持续有效。"}' \
  "$TMP/company-retained-document.json"
COMPANY_RETAINED_DOCUMENT_ID="$(jq -er .id "$TMP/company-retained-document.json")" ||
  fail "retained company knowledge document id missing"

wait_for_knowledge_document "$PRODUCT_RETAINED_DOCUMENT_ID" "$TMP/product-retained-content.json"
wait_for_knowledge_document "$PRODUCT_DELETED_DOCUMENT_ID" "$TMP/product-disposable-content.json"
wait_for_knowledge_document "$ALTERNATE_PRODUCT_RETAINED_DOCUMENT_ID" "$TMP/alternate-product-retained-content.json"
wait_for_knowledge_document "$COMPANY_RETAINED_DOCUMENT_ID" "$TMP/company-retained-content.json"

upload_status="$(curl -sS -X POST "$BASE_URL/api/v1/bids/$PROJECT_ID/documents" \
  -H "$AUTH" -H "Idempotency-Key: $(new_id)" \
  -F "file=@$TMP/tender.pdf;type=application/pdf" -o "$TMP/upload-pdf.json" -w '%{http_code}')" ||
  fail "upload real PDF tender fixture"
case "$upload_status" in 200 | 201 | 202) ;; *) fail "upload real PDF tender fixture returned HTTP $upload_status" ;; esac
PDF_DOCUMENT_ID="$(jq -er .id "$TMP/upload-pdf.json")" || fail "PDF document id missing"

upload_status="$(curl -sS -X POST "$BASE_URL/api/v1/bids/$PROJECT_ID/documents" \
  -H "$AUTH" -H "Idempotency-Key: $(new_id)" \
  -F "file=@$TMP/tender.docx;type=application/vnd.openxmlformats-officedocument.wordprocessingml.document" \
  -o "$TMP/upload-docx.json" -w '%{http_code}')" || fail "upload real DOCX tender fixture"
case "$upload_status" in 200 | 201 | 202) ;; *) fail "upload real DOCX tender fixture returned HTTP $upload_status" ;; esac
DOCX_DOCUMENT_ID="$(jq -er .id "$TMP/upload-docx.json")" || fail "DOCX document id missing"

# The real worker must convert, section, route, and atomically publish both fixtures.
for _ in $(seq 1 180); do
  get_json "/api/v1/bids/$PROJECT_ID/documents" "$TMP/documents.json"
  pdf_status="$(jq -r --arg id "$PDF_DOCUMENT_ID" '.documents[] | select(.id==$id) | .parse_status' "$TMP/documents.json")"
  docx_status="$(jq -r --arg id "$DOCX_DOCUMENT_ID" '.documents[] | select(.id==$id) | .parse_status' "$TMP/documents.json")"
  [ "$pdf_status" != "failed" ] || fail "PDF tender conversion failed"
  [ "$docx_status" != "failed" ] || fail "DOCX tender conversion failed"
  get_json "/api/v1/bids/$PROJECT_ID/clauses?include_history=false" "$TMP/clauses.json"
  clause_count="$(jq '[.clauses[] | select(.status=="draft")] | length' "$TMP/clauses.json")"
  if [ "$pdf_status" = "completed" ] && [ "$docx_status" = "completed" ] && [ "$clause_count" -ge 5 ]; then
    break
  fi
  sleep 1
done
[ "$(jq '[.clauses[] | select(.status=="draft")] | length' "$TMP/clauses.json")" -ge 5 ] ||
  fail "real tender publication did not produce the expected draft clauses"
for kind in technical qualification procedural evaluation; do
  jq -e --arg kind "$kind" '.clauses[] | select(.kind==$kind and .status=="draft")' "$TMP/clauses.json" >/dev/null ||
    fail "KindRouter did not publish $kind"
done
EVALUATION_CLAUSE_ID="$(jq -er '[.clauses[] | select(
  .kind=="evaluation" and .status=="draft"
  and (.text | contains("评分标准要求技术响应得分权重为30%")))][0].id' "$TMP/clauses.json")" ||
  fail "evaluation clause id missing"
EVALUATION_CLAUSE_TEXT="$(jq -er --arg id "$EVALUATION_CLAUSE_ID" '.clauses[] | select(.id==$id) | .text' "$TMP/clauses.json")" ||
  fail "evaluation clause text missing"
printf '%s' "$EVALUATION_CLAUSE_TEXT" | grep -q '评分标准要求技术响应得分权重为30%' ||
  fail "evaluation clause did not preserve the acceptance marker text"

while IFS=$'\t' read -r clause_id revision; do
  json_request PATCH "/api/v1/bids/$PROJECT_ID/clauses/$clause_id" \
    "{\"action\":\"confirm\",\"expected_revision\":$revision,\"patch\":{}}" \
    "$TMP/confirm-$clause_id.json"
done < <(jq -r '.clauses[] | select(.status=="draft") | [.id,.revision] | @tsv' "$TMP/clauses.json")

get_json "/api/v1/bids/$PROJECT_ID/clauses?include_history=false" "$TMP/clauses-confirmed.json"
jq -e --arg id "$EVALUATION_CLAUSE_ID" '.clauses[] | select(.id==$id and .kind=="evaluation" and .status=="confirmed")' \
  "$TMP/clauses-confirmed.json" >/dev/null || fail "evaluation clause was not confirmed"

get_json "/api/v1/bids/$PROJECT_ID/facts" "$TMP/facts-suggested.json"
FACT_ACCEPTED_CANDIDATE_ID="$(jq -er '[.suggestions[] | select(.field=="budget_amount")][0].id' "$TMP/facts-suggested.json")" ||
  fail "budget fact suggestion missing"
FACT_SUGGESTED_VALUE="$(jq -cer --arg id "$FACT_ACCEPTED_CANDIDATE_ID" '.suggestions[] | select(.id==$id) | .typed_value' "$TMP/facts-suggested.json")" ||
  fail "budget fact suggestion value missing"
FACT_REVISION="$(jq -er .project_facts.revision "$TMP/facts-suggested.json")"
json_request POST "/api/v1/bids/$PROJECT_ID/facts" \
  "{\"action\":\"accept\",\"expected_fact_revision\":$FACT_REVISION,\"candidate_id\":\"$FACT_ACCEPTED_CANDIDATE_ID\"}" \
  "$TMP/fact-accept.json"
get_json "/api/v1/bids/$PROJECT_ID/facts" "$TMP/facts-accepted.json"
FACT_REVISION="$(jq -er .project_facts.revision "$TMP/facts-accepted.json")"
json_request POST "/api/v1/bids/$PROJECT_ID/facts" \
  "{\"action\":\"set\",\"expected_fact_revision\":$FACT_REVISION,\"field\":\"budget_amount\",\"typed_value\":{\"amount\":\"1200.00\",\"currency_code\":\"CNY\"},\"reason\":\"运行验收人工修订\"}" \
  "$TMP/fact-revise.json"
get_json "/api/v1/bids/$PROJECT_ID/facts" "$TMP/facts-revised.json"
jq -e '.project_facts.budget_amount | tonumber == 1200' "$TMP/facts-revised.json" >/dev/null ||
  fail "accepted fact was not manually revised"

json_request POST "/api/v1/bids/$PROJECT_ID/matching/schedule" '{}' "$TMP/match-schedule.json"
for _ in $(seq 1 180); do
  get_json "/api/v1/bids/$PROJECT_ID/matching" "$TMP/matching.json"
  routes="$(jq '.routes | length' "$TMP/matching.json")"
  reports="$(jq '.reports | length' "$TMP/matching.json")"
  [ "$routes" -ge 2 ] && [ "$reports" -ge "$routes" ] && break
  sleep 1
done
[ "$(jq '.reports | length' "$TMP/matching.json")" -ge "$(jq '.routes | length' "$TMP/matching.json")" ] ||
  fail "two-route matching publication did not complete"
TECHNICAL_ROUTE_COUNT="$(jq '[.routes[] | select(.route_kind=="technical")] | length' "$TMP/matching.json")"
[ "$TECHNICAL_ROUTE_COUNT" -ge 1 ] || fail "technical matching route missing"
MATCHING_REPORT_IDENTITIES="$(jq -c '[.reports[] | {id,route_id,generation,content_sha256}] | sort_by(.route_id)' "$TMP/matching.json")"

SUPPORTED_TECHNICAL_ROUTE_COUNT=0
MULTI_PICK_REQUIREMENT_COUNT=0
while IFS= read -r route_id; do
  get_json "/api/v1/bids/$PROJECT_ID/matching/routes/$route_id/pick-set" \
    "$TMP/pick-set-$route_id-before.json"
  jq -e '.route_kind=="technical"' "$TMP/pick-set-$route_id-before.json" >/dev/null ||
    fail "route $route_id is not technical"
  source_report_id="$(jq -er .source_report_artifact_id "$TMP/pick-set-$route_id-before.json")"
  report_sha="$(jq -er .report_sha256 "$TMP/pick-set-$route_id-before.json")"
  pick_revision="$(jq -er .revision "$TMP/pick-set-$route_id-before.json")"
  supported_count="$(jq '.supported_candidates | length' "$TMP/pick-set-$route_id-before.json")"
  if [ "$supported_count" -gt 0 ]; then
    SUPPORTED_TECHNICAL_ROUTE_COUNT=$((SUPPORTED_TECHNICAL_ROUTE_COUNT + 1))
    pick_items="$(jq -c '[.supported_candidates | group_by(.requirement_artifact_id)[] | .[0:2][] |
      {requirement_artifact_id,candidate_artifact_id}] | if length > 0 then . + [.[0]] else . end' \
      "$TMP/pick-set-$route_id-before.json")"
  else
    jq -e '.quality_status=="review" and any((.reason_codes // [])[];
      . == "UNRESOLVED" or . == "INSUFFICIENT" or . == "NO_EVIDENCE")' \
      "$TMP/pick-set-$route_id-before.json" >/dev/null ||
      fail "technical route $route_id has no supported candidates without a valid review reason"
    pick_items='[]'
  fi
  pick_body="$(jq -cn \
    --arg source_report_artifact_id "$source_report_id" \
    --arg report_sha256 "$report_sha" \
    --argjson expected_revision "$pick_revision" \
    --argjson items "$pick_items" \
    '{source_report_artifact_id:$source_report_artifact_id,report_sha256:$report_sha256,
      expected_revision:$expected_revision,items:$items}')"
  json_request PUT "/api/v1/bids/$PROJECT_ID/matching/routes/$route_id/pick-set" \
    "$pick_body" "$TMP/pick-receipt-$route_id.json"
  get_json "/api/v1/bids/$PROJECT_ID/matching/routes/$route_id/pick-set" \
    "$TMP/pick-set-$route_id-after.json"
  jq -e --argjson expected "$pick_items" \
    '. as $actual |
      ($expected | unique_by([.requirement_artifact_id, .candidate_artifact_id])) as $canonical_expected |
      ($actual.items | length) == ($canonical_expected | length)
      and all($canonical_expected[]; . as $wanted |
        any($actual.items[];
          .requirement_artifact_id == $wanted.requirement_artifact_id
          and .candidate_artifact_id == $wanted.candidate_artifact_id))
      and ($actual.revision > 0)' "$TMP/pick-set-$route_id-after.json" >/dev/null ||
    fail "technical route $route_id did not freeze the manual picks"
  jq -e '([.items | group_by(.requirement_artifact_id)[] | length] | all(.[]; . <= 2))' \
    "$TMP/pick-set-$route_id-after.json" >/dev/null ||
    fail "technical route $route_id froze more than two candidates for one requirement"
  route_multi_pick_count="$(jq '[.items | group_by(.requirement_artifact_id)[] | select(length == 2)] | length' \
    "$TMP/pick-set-$route_id-after.json")"
  MULTI_PICK_REQUIREMENT_COUNT=$((MULTI_PICK_REQUIREMENT_COUNT + route_multi_pick_count))
done < <(jq -r '.routes[] | select(.route_kind=="technical") | .route_id' "$TMP/matching.json")
[ "$SUPPORTED_TECHNICAL_ROUTE_COUNT" -ge 1 ] ||
  fail "no technical route exercised supported candidate selection"
[ "$MULTI_PICK_REQUIREMENT_COUNT" -ge 1 ] ||
  fail "no technical requirement exercised two persisted manual picks"

jq -s '[.[] | {route_id,report_sha256,revision,items,supported_candidates}]' \
  "$TMP"/pick-set-*-after.json >"$TMP/technical-picks.json"
[ "$(jq 'length' "$TMP/technical-picks.json")" -eq "$TECHNICAL_ROUTE_COUNT" ] ||
  fail "not every technical route has a manual pick set"

# Before quote finalization, DOCX must be renderable and must freeze the fixed quote placeholder.
json_request POST "/api/v1/bids/$PROJECT_ID/submission/manifests" '{"format":"docx"}' "$TMP/draft-manifest.json"
DRAFT_MANIFEST_ID="$(jq -er .manifest_id "$TMP/draft-manifest.json")" || fail "draft manifest id missing"
DRAFT_MANIFEST_SHA="$(jq -er .content_sha256 "$TMP/draft-manifest.json")" || fail "draft manifest hash missing"
json_request POST "/api/v1/bids/$PROJECT_ID/submission/manifests/$DRAFT_MANIFEST_ID/render" \
  "{\"expected_manifest_sha256\":\"$DRAFT_MANIFEST_SHA\"}" "$TMP/draft-render.json"
jq -e --arg manifest_id "$DRAFT_MANIFEST_ID" '
  .status == "queued"
  and .manifest_id == $manifest_id
  and (.render_job_id | type == "string" and length > 0)
' "$TMP/draft-render.json" >/dev/null || fail "DOCX render was not queued"
DRAFT_RENDER_JOB_ID="$(jq -er .render_job_id "$TMP/draft-render.json")" || fail "DOCX render job id missing"
DRAFT_OUTPUT_ID="$(wait_for_render_job \
  "$PROJECT_ID" "$DRAFT_MANIFEST_ID" "$DRAFT_RENDER_JOB_ID" docx "$TMP/draft-render-job.json")"
download_output "$PROJECT_ID" "$DRAFT_OUTPUT_ID" "$TMP/process.docx" "$TMP/process.headers"
python3 - "$TMP/process.docx" <<'PY' || fail "DOCX quote placeholder missing"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    xml = archive.read("word/document.xml").decode("utf-8")
assert "报价尚未最终确认" in xml
PY

# Formal PDF must remain blocked until the required profiles, procedural
# decisions, eligible quote snapshot, and generated parts are complete.
blocked_pdf_status="$(curl -sS -X POST \
  "$BASE_URL/api/v1/bids/$PROJECT_ID/submission/manifests" \
  -H "$AUTH" -H 'content-type: application/json' \
  -H "Idempotency-Key: $(new_id)" \
  --data '{"format":"pdf"}' -o "$TMP/blocked-pdf-manifest.json" -w '%{http_code}')" ||
  fail "request blocked PDF manifest"
[ "$blocked_pdf_status" = "400" ] ||
  fail "incomplete formal PDF returned HTTP $blocked_pdf_status instead of 400"
jq -e '.error.code == "SUBMISSION_GATE_REJECTED"' \
  "$TMP/blocked-pdf-manifest.json" >/dev/null ||
  fail "incomplete formal PDF was not rejected by SubmissionGateV1"

json_request PUT "/api/v1/bids/$PROJECT_ID/company-profile" \
  '{"expected_revision":0,"legal_name":"示例网络安全有限公司","unified_social_credit_code":"91310000MA00000001","registered_address":"上海市浦东新区示例路1号","legal_representative":"张三","contact_name":"李四","contact_phone":"13800000000","contact_email":"bid@example.test"}' \
  "$TMP/company-profile.json"
json_request PUT "/api/v1/bids/$PROJECT_ID/submission-profile" \
  "{\"expected_revision\":0,\"buyer_name\":\"示例采购人\",\"project_code\":\"KB-V1-ACCEPTANCE\",\"authorized_representative\":\"李四\",\"submission_date\":\"$SUBMISSION_DATE\",\"submission_place\":\"上海\",\"seal_confirmed\":true,\"signature_confirmed\":true}" \
  "$TMP/submission-profile.json"

get_json "/api/v1/bids/$PROJECT_ID/procedural-requirements" "$TMP/procedural.json"
[ "$(jq '.classifications | length' "$TMP/procedural.json")" -ge 1 ] ||
  fail "procedural classification missing"
ATTACHMENT_CLASSIFICATION_ID="$(jq -er '[.classifications[] |
  select(.lifecycle_status=="current" and .effective_requirement_kind=="authorization_support")][0].id' \
  "$TMP/procedural.json")" || fail "authorization-support procedural classification missing"
attachment_upload_status="$(curl -sS -X POST "$BASE_URL/api/v1/bids/$PROJECT_ID/attachments" \
  -H "$AUTH" -H "Idempotency-Key: $(new_id)" \
  -F 'kind=authorization_support' \
  -F "file=@$TMP/authorization-support.pdf;filename=authorization-support.pdf;type=application/pdf" \
  -o "$TMP/attachment-upload.json" -w '%{http_code}')" || fail "upload procedural attachment"
case "$attachment_upload_status" in 200 | 201 | 202) ;;
  *) fail "upload procedural attachment returned HTTP $attachment_upload_status" ;;
esac
ATTACHMENT_ID="$(jq -er .id "$TMP/attachment-upload.json")" || fail "attachment id missing"
ATTACHMENT_REVISION="$(jq -er .revision "$TMP/attachment-upload.json")"
json_request POST "/api/v1/bids/$PROJECT_ID/attachments/$ATTACHMENT_ID/validate" \
  "{\"expected_revision\":$ATTACHMENT_REVISION}" "$TMP/attachment-validate.json"
ATTACHMENT_REVISION="$(jq -er 'select(.validation_status=="valid") | .revision' "$TMP/attachment-validate.json")" ||
  fail "attachment validation did not publish valid status"
json_request POST "/api/v1/bids/$PROJECT_ID/attachments/$ATTACHMENT_ID/confirm" \
  "{\"expected_revision\":$ATTACHMENT_REVISION}" "$TMP/attachment-confirm.json"
ATTACHMENT_REVISION="$(jq -er 'select(.status=="confirmed") | .revision' "$TMP/attachment-confirm.json")" ||
  fail "attachment confirmation did not publish confirmed status"
get_json "/api/v1/bids/$PROJECT_ID/attachments" "$TMP/attachments.json"
ATTACHMENT_OBJECT_REF="$(jq -er --arg id "$ATTACHMENT_ID" '.attachments[] | select(.id==$id) | .object_ref' "$TMP/attachments.json")" ||
  fail "attachment object_ref missing"
ATTACHMENT_DIGEST="$(jq -er --arg id "$ATTACHMENT_ID" '.attachments[] | select(.id==$id) | .content_sha256' "$TMP/attachments.json")" ||
  fail "attachment digest missing"
json_request POST "/api/v1/bids/$PROJECT_ID/procedural-requirements/$ATTACHMENT_CLASSIFICATION_ID/resolve" \
  "{\"resolution\":\"satisfied_by_attachment\",\"attachment_id\":\"$ATTACHMENT_ID\"}" \
  "$TMP/resolve-$ATTACHMENT_CLASSIFICATION_ID.json"

while IFS=$'\t' read -r classification_id effective_kind; do
  case "$effective_kind" in
    authorization_support) continue ;;
    confirmation) resolution='{"resolution":"confirmed_by_user"}' ;;
    *) resolution='{"resolution":"not_applicable","reason":"运行验收明确确认本样例无需额外附件"}' ;;
  esac
  json_request POST "/api/v1/bids/$PROJECT_ID/procedural-requirements/$classification_id/resolve" \
    "$resolution" "$TMP/resolve-$classification_id.json"
done < <(jq -r '.classifications[] | select(.lifecycle_status=="current") | [.id,.effective_requirement_kind] | @tsv' "$TMP/procedural.json")

json_request POST "/api/v1/bids/$PROJECT_ID/quote/draft" \
  '{"tax_mode":"tax_inclusive","title":"投标报价一览表","notes":"运行验收人工报价"}' \
  "$TMP/quote-draft.json"
EDIT_VERSION="$(jq -er .edit_version "$TMP/quote-draft.json")" || fail "quote draft edit version missing"
LINE_ID="$(new_id)"
json_request PUT "/api/v1/bids/$PROJECT_ID/quote/lines/$LINE_ID" \
  "{\"expected_edit_version\":$EDIT_VERSION,\"ordinal\":1,\"description\":\"网络安全产品与服务\",\"pricing_mode\":\"lump_sum\",\"quantity\":null,\"unit\":null,\"unit_price\":null,\"entered_amount\":\"1000.00\",\"tax_rate\":\"0.060000\",\"user_confirmed\":true}" \
  "$TMP/quote-line.json"
get_json "/api/v1/bids/$PROJECT_ID/quote" "$TMP/quote-before-finalize.json"
EDIT_VERSION="$(jq -er .edit_version "$TMP/quote-before-finalize.json")" || fail "edited quote version missing"
get_json "/api/v1/bids/$PROJECT_ID" "$TMP/bid-detail.json"
FACT_REVISION="$(jq -er .project.fact_revision "$TMP/bid-detail.json")"
CEILING_REVISION="$(jq -er .project.ceiling_revision "$TMP/bid-detail.json")"
CEILING_SHA="$(jq -er .project.ceiling_identity_sha256 "$TMP/bid-detail.json")"
PRICING_REVISION="$(jq -er '.clause_sets[] | select(.set_kind=="pricing") | .revision' "$TMP/bid-detail.json")"
PRICING_SHA="$(jq -er '.clause_sets[] | select(.set_kind=="pricing") | .content_sha256' "$TMP/bid-detail.json")"
json_request POST "/api/v1/bids/$PROJECT_ID/quote/finalize" \
  "{\"expected_edit_version\":$EDIT_VERSION,\"expected_fact_revision\":$FACT_REVISION,\"expected_ceiling_revision\":$CEILING_REVISION,\"expected_ceiling_identity_sha256\":\"$CEILING_SHA\",\"expected_pricing_revision\":$PRICING_REVISION,\"expected_pricing_set_sha256\":\"$PRICING_SHA\",\"no_ceiling_reviewed\":true,\"no_ceiling_reason\":\"招标样例未设置最高限价，运行验收已人工复核\"}" \
  "$TMP/quote-finalize.json"
get_json "/api/v1/bids/$PROJECT_ID/quote" "$TMP/quote-final.json"
[ "$(jq -r .eligibility "$TMP/quote-final.json")" = "eligible" ] ||
  fail "final quote is not eligible"
FIRST_QUOTE_SNAPSHOT_ID="$(jq -er .snapshot_id "$TMP/quote-final.json")"

json_request POST "/api/v1/bids/$PROJECT_ID/quote/reopen" \
  "{\"expected_snapshot_id\":\"$FIRST_QUOTE_SNAPSHOT_ID\",\"expected_fact_revision\":$FACT_REVISION,\"expected_pricing_revision\":$PRICING_REVISION}" \
  "$TMP/quote-reopen.json"
get_json "/api/v1/bids/$PROJECT_ID/quote" "$TMP/quote-reopened.json"
jq -e '.edit_version==0 and .active_finalized_snapshot_id==null' "$TMP/quote-reopened.json" >/dev/null ||
  fail "quote reopen did not publish an editable draft"
EDIT_VERSION="$(jq -er .edit_version "$TMP/quote-reopened.json")"
get_json "/api/v1/bids/$PROJECT_ID" "$TMP/bid-detail-refinalize.json"
FACT_REVISION="$(jq -er .project.fact_revision "$TMP/bid-detail-refinalize.json")"
CEILING_REVISION="$(jq -er .project.ceiling_revision "$TMP/bid-detail-refinalize.json")"
CEILING_SHA="$(jq -er .project.ceiling_identity_sha256 "$TMP/bid-detail-refinalize.json")"
PRICING_REVISION="$(jq -er '.clause_sets[] | select(.set_kind=="pricing") | .revision' "$TMP/bid-detail-refinalize.json")"
PRICING_SHA="$(jq -er '.clause_sets[] | select(.set_kind=="pricing") | .content_sha256' "$TMP/bid-detail-refinalize.json")"
json_request POST "/api/v1/bids/$PROJECT_ID/quote/finalize" \
  "{\"expected_edit_version\":$EDIT_VERSION,\"expected_fact_revision\":$FACT_REVISION,\"expected_ceiling_revision\":$CEILING_REVISION,\"expected_ceiling_identity_sha256\":\"$CEILING_SHA\",\"expected_pricing_revision\":$PRICING_REVISION,\"expected_pricing_set_sha256\":\"$PRICING_SHA\",\"no_ceiling_reviewed\":true,\"no_ceiling_reason\":\"招标样例未设置最高限价，运行验收已人工复核\"}" \
  "$TMP/quote-refinalize.json"
get_json "/api/v1/bids/$PROJECT_ID/quote" "$TMP/quote-refinal.json"
[ "$(jq -r .eligibility "$TMP/quote-refinal.json")" = "eligible" ] ||
  fail "refinalized quote is not eligible"
QUOTE_SNAPSHOT_ID="$(jq -er .snapshot_id "$TMP/quote-refinal.json")"
[ "$QUOTE_SNAPSHOT_ID" != "$FIRST_QUOTE_SNAPSHOT_ID" ] ||
  fail "quote refinalize did not publish a new snapshot"

# Freeze a real image occurrence into the formal manifest. This object remains
# protected by the immutable manifest independently from the disposable
# procedural attachment exercised by the Compose hook.
shot_upload_status="$(curl -sS -X POST "$BASE_URL/api/v1/bids/$PROJECT_ID/shots/artifacts" \
  -H "$AUTH" -H "Idempotency-Key: $(new_id)" \
  -F "file=@$TMP/shot.png;filename=topology.png;type=image/png" \
  -o "$TMP/shot-upload.json" -w '%{http_code}')" || fail "upload render shot artifact"
[ "$shot_upload_status" = "201" ] ||
  fail "upload render shot artifact returned HTTP $shot_upload_status"
SHOT_ARTIFACT_ID="$(jq -er .shot_artifact_id "$TMP/shot-upload.json")" || fail "shot artifact id missing"
SHOT_OBJECT_REF="$(jq -er .object_ref "$TMP/shot-upload.json")" || fail "shot object_ref missing"
SHOT_DIGEST="$(jq -er .digest "$TMP/shot-upload.json")" || fail "shot digest missing"
get_json "/api/v1/bids/$PROJECT_ID/shots" "$TMP/shots-before.json"
SHOT_SET_REVISION="$(jq -er '.shot_set.revision // 0' "$TMP/shots-before.json")"
json_request PUT "/api/v1/bids/$PROJECT_ID/shots" \
  "$(jq -cn --argjson revision "$SHOT_SET_REVISION" --arg id "$SHOT_ARTIFACT_ID" \
    '{expected_revision:$revision,shot_artifact_ids:[$id]}')" "$TMP/shot-set.json"
SHOT_SET_ID="$(jq -er .shot_set_id "$TMP/shot-set.json")" || fail "shot set id missing"
SHOT_SET_REVISION="$(jq -er .revision "$TMP/shot-set.json")" || fail "shot set revision missing"

# Every required part is generated by the server from its typed current identities.
get_json "/api/v1/bids/$PROJECT_ID/parts" "$TMP/parts-before.json"
while IFS= read -r part_key; do
  encoded_key="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$part_key")"
  revision="$(jq -r --arg key "$part_key" '[.parts[] | select(.part_key==$key) | .content_revision][0] // 0' "$TMP/parts-before.json")"
  dependency="$(jq -r --arg key "$part_key" '[.parts[] | select(.part_key==$key) | .dependency_sha256][0] // empty' "$TMP/parts-before.json")"
  if [ -n "$dependency" ]; then
    body="{\"expected_content_revision\":$revision,\"expected_dependency_sha256\":\"$dependency\"}"
  else
    body="{\"expected_content_revision\":$revision}"
  fi
  json_request POST "/api/v1/bids/$PROJECT_ID/parts/$encoded_key/regenerate" "$body" \
    "$TMP/part-$(printf '%s' "$part_key" | tr ':/' '__').json"
done < <(jq -r '.required_part_keys[]' "$TMP/parts-before.json")

get_json "/api/v1/bids/$PROJECT_ID/gate-issues?format=pdf" "$TMP/pdf-gate.json"
[ "$(jq -r .status "$TMP/pdf-gate.json")" = "pass" ] ||
  fail "SubmissionGateV1 rejected the completed project"
jq -e '
  all(.issues[];
    .code == "PROCEDURAL_NOT_APPLICABLE"
    and (.current_identity.reason | type == "string" and length > 0)
    and (.current_identity.actor_identity | type == "string" and startswith("user:")))
' "$TMP/pdf-gate.json" >/dev/null ||
  fail "PDF gate contained an issue other than a frozen not-applicable warning"

json_request POST "/api/v1/bids/$PROJECT_ID/submission/manifests" '{"format":"pdf"}' "$TMP/pdf-manifest.json"
PDF_MANIFEST_ID="$(jq -er .manifest_id "$TMP/pdf-manifest.json")" || fail "PDF manifest id missing"
PDF_MANIFEST_SHA="$(jq -er .content_sha256 "$TMP/pdf-manifest.json")" || fail "PDF manifest hash missing"
json_request POST "/api/v1/bids/$PROJECT_ID/submission/manifests/$PDF_MANIFEST_ID/render" \
  "{\"expected_manifest_sha256\":\"$PDF_MANIFEST_SHA\"}" "$TMP/pdf-render.json"
jq -e --arg manifest_id "$PDF_MANIFEST_ID" '
  .status == "queued"
  and .manifest_id == $manifest_id
  and (.render_job_id | type == "string" and length > 0)
' "$TMP/pdf-render.json" >/dev/null || fail "PDF render was not queued"
PDF_RENDER_JOB_ID="$(jq -er .render_job_id "$TMP/pdf-render.json")" || fail "PDF render job id missing"
PDF_OUTPUT_ID="$(wait_for_render_job \
  "$PROJECT_ID" "$PDF_MANIFEST_ID" "$PDF_RENDER_JOB_ID" pdf "$TMP/pdf-render-job.json")"
download_output "$PROJECT_ID" "$PDF_OUTPUT_ID" "$TMP/formal.pdf" "$TMP/formal.headers"
[ "$(head -c 5 "$TMP/formal.pdf")" = "%PDF-" ] || fail "formal output is not a PDF"

DOCX_SHA="$(sha256sum "$TMP/process.docx" | awk '{print $1}')"
PDF_SHA="$(sha256sum "$TMP/formal.pdf" | awk '{print $1}')"

# Delete only the duplicate live knowledge document. Frozen report identities,
# picks, and already-published outputs must remain replayable.
delete_status="$(curl -sS -X DELETE "$BASE_URL/api/v1/documents/$PRODUCT_DELETED_DOCUMENT_ID" \
  -H "$AUTH" -o "$TMP/knowledge-delete.json" -w '%{http_code}')" ||
  fail "delete disposable live knowledge document"
[ "$delete_status" = "202" ] ||
  fail "delete disposable live knowledge document returned HTTP $delete_status"
knowledge_get_status=000
for _ in $(seq 1 180); do
  knowledge_get_status="$(curl -sS "$BASE_URL/api/v1/documents/$PRODUCT_DELETED_DOCUMENT_ID" \
    -H "$AUTH" -o "$TMP/deleted-knowledge-get.json" -w '%{http_code}')" ||
    fail "inspect disposable knowledge document deletion"
  [ "$knowledge_get_status" = "404" ] && break
  sleep 1
done
[ "$knowledge_get_status" = "404" ] ||
  fail "disposable live knowledge document was not deleted within 180 seconds"

get_json "/api/v1/bids/$PROJECT_ID/matching" "$TMP/matching-replayed.json"
jq -e --argjson expected "$MATCHING_REPORT_IDENTITIES" \
  '([.reports[] | {id,route_id,generation,content_sha256}] | sort_by(.route_id)) == $expected' \
  "$TMP/matching-replayed.json" >/dev/null ||
  fail "frozen matching report identities changed after live knowledge deletion"
HISTORICAL_REPORT_COUNT=0
HISTORICAL_REPORT_WITH_DELETED_SOURCE_COUNT=0
while read -r historical_report_id historical_report_sha; do
  get_json "/api/v1/bids/$PROJECT_ID/matching/reports/$historical_report_id" \
    "$TMP/historical-report-$historical_report_id.json"
  jq -e --arg report_id "$historical_report_id" --arg report_sha "$historical_report_sha" '
    .id == $report_id
    and .content_sha256 == $report_sha
    and (.canonical_payload | type == "string")
    and ((.canonical_payload | fromjson) == .payload)
    and .payload.report_id == $report_id
    and (.payload.source_artifacts | type == "array")
  ' "$TMP/historical-report-$historical_report_id.json" >/dev/null ||
    fail "historical matching report artifact identity changed after live knowledge deletion"
  replayed_report_sha="$(jq -jr .canonical_payload \
    "$TMP/historical-report-$historical_report_id.json" | sha256sum | awk '{print $1}')"
  [ "$replayed_report_sha" = "$historical_report_sha" ] ||
    fail "historical matching report canonical bytes no longer match the frozen hash"
  HISTORICAL_REPORT_COUNT=$((HISTORICAL_REPORT_COUNT + 1))
  if jq -e --arg document_id "$PRODUCT_DELETED_DOCUMENT_ID" '
    any(.payload.source_artifacts[]?; .document_id == $document_id)
  ' "$TMP/historical-report-$historical_report_id.json" >/dev/null; then
    HISTORICAL_REPORT_WITH_DELETED_SOURCE_COUNT=$((HISTORICAL_REPORT_WITH_DELETED_SOURCE_COUNT + 1))
  fi
done < <(jq -r '.[] | "\(.id) \(.content_sha256)"' <<<"$MATCHING_REPORT_IDENTITIES")
[ "$HISTORICAL_REPORT_COUNT" -eq "$(jq 'length' <<<"$MATCHING_REPORT_IDENTITIES")" ] ||
  fail "not every frozen matching report was replayed through the immutable artifact API"
[ "$HISTORICAL_REPORT_WITH_DELETED_SOURCE_COUNT" -ge 1 ] ||
  fail "historical report replay did not include the deleted live knowledge source"
download_output "$PROJECT_ID" "$DRAFT_OUTPUT_ID" "$TMP/process-replayed.docx" "$TMP/process-replayed.headers"
download_output "$PROJECT_ID" "$PDF_OUTPUT_ID" "$TMP/formal-replayed.pdf" "$TMP/formal-replayed.headers"
[ "$(sha256sum "$TMP/process-replayed.docx" | awk '{print $1}')" = "$DOCX_SHA" ] ||
  fail "historical DOCX output changed after live knowledge deletion"
[ "$(sha256sum "$TMP/formal-replayed.pdf" | awk '{print $1}')" = "$PDF_SHA" ] ||
  fail "historical PDF output changed after live knowledge deletion"

# Compose-only orchestration (worker restart/recovery, router promotion markers,
# attachment reference/retention checks) is injected here. Secrets are passed in
# the child environment, never as argv. A configured hook must emit JSON evidence.
BEFORE_END_HOOK_CONFIGURED=false
BEFORE_END_HOOK_STATUS=not_configured
jq -n '{status:"not_configured",reason:"ACCEPTANCE_BEFORE_END_HOOK is empty"}' \
  >"$TMP/before-end-hook-evidence.json"
if [ -n "$ACCEPTANCE_BEFORE_END_HOOK" ]; then
  [ -x "$ACCEPTANCE_BEFORE_END_HOOK" ] ||
    fail "ACCEPTANCE_BEFORE_END_HOOK is not executable: $ACCEPTANCE_BEFORE_END_HOOK"
  BEFORE_END_HOOK_CONFIGURED=true
  jq -n '{status:"awaiting_hook"}' >"$TMP/before-end-hook-evidence.json"
  ACCEPTANCE_BASE_URL="$BASE_URL" \
  ACCEPTANCE_PROJECT_ID="$PROJECT_ID" \
  ACCEPTANCE_AUTH_TOKEN="$TOKEN" \
  ACCEPTANCE_ATTACHMENT_ID="$ATTACHMENT_ID" \
  ACCEPTANCE_ATTACHMENT_REVISION="$ATTACHMENT_REVISION" \
  ACCEPTANCE_ATTACHMENT_OBJECT_REF="$ATTACHMENT_OBJECT_REF" \
  ACCEPTANCE_ATTACHMENT_DIGEST="$ATTACHMENT_DIGEST" \
  ACCEPTANCE_ATTACHMENT_CLASSIFICATION_ID="$ATTACHMENT_CLASSIFICATION_ID" \
  ACCEPTANCE_EVALUATION_CLAUSE_ID="$EVALUATION_CLAUSE_ID" \
  ACCEPTANCE_EVALUATION_CLAUSE_TEXT="$EVALUATION_CLAUSE_TEXT" \
  ACCEPTANCE_PDF_MANIFEST_ID="$PDF_MANIFEST_ID" \
  ACCEPTANCE_PDF_OUTPUT_ID="$PDF_OUTPUT_ID" \
  ACCEPTANCE_PDF_SHA256="$PDF_SHA" \
  ACCEPTANCE_HOOK_EVIDENCE_FILE="$TMP/before-end-hook-evidence.json" \
    "$ACCEPTANCE_BEFORE_END_HOOK" || fail "before-end Compose acceptance hook failed"
  jq -e 'type=="object" and .status=="passed"' "$TMP/before-end-hook-evidence.json" >/dev/null ||
    fail "before-end Compose acceptance hook did not emit passed JSON evidence"
  BEFORE_END_HOOK_STATUS=passed
fi
BEFORE_END_HOOK_EVIDENCE="$(jq -c . "$TMP/before-end-hook-evidence.json")"

get_json "/api/v1/bids/$PROJECT_ID" "$TMP/bid-before-end.json"
END_FACT_REVISION="$(jq -er .project.fact_revision "$TMP/bid-before-end.json")"
json_request POST "/api/v1/bids/$PROJECT_ID" \
  "{\"expected_fact_revision\":$END_FACT_REVISION}" "$TMP/project-ended.json"
jq -e '.status=="ended"' "$TMP/project-ended.json" >/dev/null ||
  fail "project did not enter ended state"

# Prove stable rejection twice for both new publication and new export.
for attempt in 1 2; do
  ended_publication_status="$(curl -sS -X POST "$BASE_URL/api/v1/bids/$PROJECT_ID/documents" \
    -H "$AUTH" -H "Idempotency-Key: $(new_id)" \
    -F "file=@$TMP/tender.docx;type=application/vnd.openxmlformats-officedocument.wordprocessingml.document" \
    -o "$TMP/ended-publication-$attempt.json" -w '%{http_code}')" ||
    fail "request ended publication rejection attempt $attempt"
  assert_error "$ended_publication_status" 409 ENDED "$TMP/ended-publication-$attempt.json" \
    "ended publication attempt $attempt"

  ended_export_status="$(curl -sS -X POST "$BASE_URL/api/v1/bids/$PROJECT_ID/submission/manifests" \
    -H "$AUTH" -H 'content-type: application/json' -H "Idempotency-Key: $(new_id)" \
    --data '{"format":"pdf"}' -o "$TMP/ended-export-$attempt.json" -w '%{http_code}')" ||
    fail "request ended export rejection attempt $attempt"
  assert_error "$ended_export_status" 409 ENDED "$TMP/ended-export-$attempt.json" \
    "ended export attempt $attempt"
done

download_output "$PROJECT_ID" "$PDF_OUTPUT_ID" "$TMP/formal-after-end.pdf" "$TMP/formal-after-end.headers"
[ "$(sha256sum "$TMP/formal-after-end.pdf" | awk '{print $1}')" = "$PDF_SHA" ] ||
  fail "historical PDF output changed after project end"

PDF_FIXTURE_SHA="$(sha256sum "$TMP/tender.pdf" | awk '{print $1}')"
DOCX_FIXTURE_SHA="$(sha256sum "$TMP/tender.docx" | awk '{print $1}')"
SCHEMA_MANIFEST_SHA="$(sha256sum "$ROOT/deploy/first-launch/migration-manifest.toml" | awk '{print $1}')"
GIT_SHA="${ACCEPTANCE_GIT_SHA:-$(git -C "$ROOT" rev-parse HEAD)}"
GIT_DIFF_SHA256="${ACCEPTANCE_GIT_DIFF_SHA256:-$(git -C "$ROOT" diff --binary HEAD | sha256sum | awk '{print $1}')}"
if [ -n "${ACCEPTANCE_GIT_DIRTY:-}" ]; then
  GIT_DIRTY="$ACCEPTANCE_GIT_DIRTY"
elif [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" ]; then
  GIT_DIRTY=true
else
  GIT_DIRTY=false
fi
[ "$GIT_DIRTY" = true ] || [ "$GIT_DIRTY" = false ] ||
  fail "ACCEPTANCE_GIT_DIRTY must be true or false"
OUTPUTS="$(curl -fsS "$BASE_URL/api/v1/bids/$PROJECT_ID/submission/outputs" -H "$AUTH")"
AUDIT_COUNT="${AUDIT_COUNT:-null}"
TECHNICAL_PICKS="$(jq -c . "$TMP/technical-picks.json")"
FACT_ACCEPT_RESPONSE="$(jq -c . "$TMP/fact-accept.json")"
FACT_REVISE_RESPONSE="$(jq -c . "$TMP/fact-revise.json")"
ATTACHMENT_STATE="$(jq -c --arg id "$ATTACHMENT_ID" '.attachments[] | select(.id==$id)' "$TMP/attachments.json")"
INITIAL_GATE_REJECTION="$(jq -c . "$TMP/blocked-pdf-manifest.json")"
FINAL_GATE="$(jq -c . "$TMP/pdf-gate.json")"
PROJECT_ENDED="$(jq -c . "$TMP/project-ended.json")"
ENDED_PUBLICATION_REJECTIONS="$(jq -s '[.[] | {http_status:409,error:.error}]' "$TMP"/ended-publication-*.json)"
ENDED_EXPORT_REJECTIONS="$(jq -s '[.[] | {http_status:409,error:.error}]' "$TMP"/ended-export-*.json)"

jq -n \
  --arg git_sha "$GIT_SHA" \
  --arg git_diff_sha256 "$GIT_DIFF_SHA256" \
  --argjson git_dirty "$GIT_DIRTY" \
  --arg schema_manifest_sha256 "$SCHEMA_MANIFEST_SHA" \
  --arg project_id "$PROJECT_ID" \
  --arg pdf_document_id "$PDF_DOCUMENT_ID" \
  --arg docx_document_id "$DOCX_DOCUMENT_ID" \
  --arg pdf_fixture_sha256 "$PDF_FIXTURE_SHA" \
  --arg docx_fixture_sha256 "$DOCX_FIXTURE_SHA" \
  --arg product_workspace_id "$PRODUCT_WORKSPACE_ID" \
  --arg product_id "$PRODUCT_ID" \
  --arg product_version_id "$PRODUCT_VERSION_ID" \
  --arg product_retained_document_id "$PRODUCT_RETAINED_DOCUMENT_ID" \
  --arg product_deleted_document_id "$PRODUCT_DELETED_DOCUMENT_ID" \
  --arg alternate_product_id "$ALTERNATE_PRODUCT_ID" \
  --arg alternate_product_version_id "$ALTERNATE_PRODUCT_VERSION_ID" \
  --arg alternate_product_retained_document_id "$ALTERNATE_PRODUCT_RETAINED_DOCUMENT_ID" \
  --arg company_workspace_id "$COMPANY_WORKSPACE_ID" \
  --arg company_product_id "$COMPANY_PRODUCT_ID" \
  --arg company_version_id "$COMPANY_VERSION_ID" \
  --arg company_retained_document_id "$COMPANY_RETAINED_DOCUMENT_ID" \
  --arg evaluation_clause_id "$EVALUATION_CLAUSE_ID" \
  --arg evaluation_clause_text "$EVALUATION_CLAUSE_TEXT" \
  --arg fact_candidate_id "$FACT_ACCEPTED_CANDIDATE_ID" \
  --argjson fact_suggested_value "$FACT_SUGGESTED_VALUE" \
  --argjson fact_accept_response "$FACT_ACCEPT_RESPONSE" \
  --argjson fact_revise_response "$FACT_REVISE_RESPONSE" \
  --argjson matching_reports "$MATCHING_REPORT_IDENTITIES" \
  --argjson technical_picks "$TECHNICAL_PICKS" \
  --arg attachment_id "$ATTACHMENT_ID" \
  --arg attachment_classification_id "$ATTACHMENT_CLASSIFICATION_ID" \
  --arg attachment_object_ref "$ATTACHMENT_OBJECT_REF" \
  --arg attachment_digest "$ATTACHMENT_DIGEST" \
  --argjson attachment_state "$ATTACHMENT_STATE" \
  --arg shot_artifact_id "$SHOT_ARTIFACT_ID" \
  --arg shot_set_id "$SHOT_SET_ID" \
  --argjson shot_set_revision "$SHOT_SET_REVISION" \
  --arg shot_object_ref "$SHOT_OBJECT_REF" \
  --arg shot_digest "$SHOT_DIGEST" \
  --arg first_quote_snapshot_id "$FIRST_QUOTE_SNAPSHOT_ID" \
  --arg quote_snapshot_id "$QUOTE_SNAPSHOT_ID" \
  --arg docx_manifest_id "$DRAFT_MANIFEST_ID" \
  --arg docx_output_id "$DRAFT_OUTPUT_ID" \
  --arg docx_sha256 "$DOCX_SHA" \
  --arg pdf_manifest_id "$PDF_MANIFEST_ID" \
  --arg pdf_manifest_sha256 "$PDF_MANIFEST_SHA" \
  --arg pdf_output_id "$PDF_OUTPUT_ID" \
  --arg pdf_sha256 "$PDF_SHA" \
  --arg runtime_mode "$ACCEPTANCE_RUNTIME_MODE" \
  --arg extract_mode "$ACCEPTANCE_EXTRACT_MODE" \
  --arg input_mode "$ACCEPTANCE_INPUT_MODE" \
  --arg docreader_mode "$ACCEPTANCE_DOCREADER_MODE" \
  --arg before_end_hook_status "$BEFORE_END_HOOK_STATUS" \
  --argjson before_end_hook_configured "$BEFORE_END_HOOK_CONFIGURED" \
  --argjson before_end_hook_evidence "$BEFORE_END_HOOK_EVIDENCE" \
  --argjson initial_gate_rejection "$INITIAL_GATE_REJECTION" \
  --argjson final_gate "$FINAL_GATE" \
  --argjson project_ended "$PROJECT_ENDED" \
  --argjson ended_publication_rejections "$ENDED_PUBLICATION_REJECTIONS" \
  --argjson ended_export_rejections "$ENDED_EXPORT_REJECTIONS" \
  --argjson outputs "$OUTPUTS" \
  --argjson audit_count "$AUDIT_COUNT" \
  '{schema_version:3,git_sha:$git_sha,git_diff_sha256:$git_diff_sha256,git_dirty:$git_dirty,
    schema_manifest_sha256:$schema_manifest_sha256,
    project_id:$project_id,
    execution:{runtime:$runtime_mode,extract:$extract_mode,input:$input_mode,
      docreader:$docreader_mode,playwright:"not-used"},
    tender_inputs:{pdf:{document_id:$pdf_document_id,sha256:$pdf_fixture_sha256,media_type:"application/pdf"},
      docx:{document_id:$docx_document_id,sha256:$docx_fixture_sha256,
        media_type:"application/vnd.openxmlformats-officedocument.wordprocessingml.document"}},
    knowledge:{product_line:{workspace_id:$product_workspace_id,product_id:$product_id,
        version_id:$product_version_id,retained_document_id:$product_retained_document_id,
        deleted_document_id:$product_deleted_document_id,deleted_live:true,
        alternate_product_id:$alternate_product_id,
        alternate_version_id:$alternate_product_version_id,
        alternate_retained_document_id:$alternate_product_retained_document_id},
      company:{workspace_id:$company_workspace_id,product_id:$company_product_id,
        version_id:$company_version_id,retained_document_id:$company_retained_document_id},
      retained_document_ids:[$product_retained_document_id,$alternate_product_retained_document_id,
        $company_retained_document_id]},
    fact:{accepted_candidate_id:$fact_candidate_id,suggested_value:$fact_suggested_value,
      accept_response:$fact_accept_response,manual_revision:$fact_revise_response},
    evaluation_clause:{id:$evaluation_clause_id,text:$evaluation_clause_text,status:"confirmed"},
    matching:{report_identities:$matching_reports,technical_picks:$technical_picks,
      replayed_after_live_document_delete:true},
    attachment:{id:$attachment_id,classification_id:$attachment_classification_id,
      object_ref:$attachment_object_ref,digest:$attachment_digest,pre_hook_state:$attachment_state},
    shot:{artifact_id:$shot_artifact_id,set_id:$shot_set_id,set_revision:$shot_set_revision,
      object_ref:$shot_object_ref,digest:$shot_digest},
    quote:{first_finalized_snapshot_id:$first_quote_snapshot_id,
      refinalized_snapshot_id:$quote_snapshot_id,reopen_refinalize_verified:true},
    gate:{initial_rejection:$initial_gate_rejection,final:$final_gate},
    compose_before_end_hook:{configured:$before_end_hook_configured,status:$before_end_hook_status,
      interface:"environment-v1",
      required_scenarios:["worker_restart_recovery","kind_router_marker_reconfirm_part_refresh",
        "attachment_release_delete_manifest_asset_protection_and_historical_pdf_replay"],
      evidence:$before_end_hook_evidence},
    ended:{response:$project_ended,publication_rejections:$ended_publication_rejections,
      export_rejections:$ended_export_rejections,historical_pdf_replayed:true},
    docx:{manifest_id:$docx_manifest_id,output_id:$docx_output_id,sha256:$docx_sha256},
    pdf:{manifest_id:$pdf_manifest_id,manifest_sha256:$pdf_manifest_sha256,output_id:$pdf_output_id,sha256:$pdf_sha256},
    audit_count:$audit_count,outputs:$outputs}' >"$EVIDENCE_DIR/evidence.json"
cp "$TMP/process.headers" "$EVIDENCE_DIR/docx-download.headers"
cp "$TMP/formal.headers" "$EVIDENCE_DIR/pdf-download.headers"
cp "$TMP/formal-after-end.headers" "$EVIDENCE_DIR/pdf-after-end-download.headers"
cp "$TMP/pdf-gate.json" "$EVIDENCE_DIR/pdf-gate.json"
cp "$TMP/technical-picks.json" "$EVIDENCE_DIR/technical-picks.json"
cp "$TMP/before-end-hook-evidence.json" "$EVIDENCE_DIR/before-end-hook-evidence.json"
cp "$TMP/tender.pdf" "$EVIDENCE_DIR/tender.pdf"
cp "$TMP/tender.docx" "$EVIDENCE_DIR/tender.docx"
cp "$TMP/authorization-support.pdf" "$EVIDENCE_DIR/authorization-support.pdf"
cp "$TMP/shot.png" "$EVIDENCE_DIR/shot.png"
cp "$TMP/process.docx" "$EVIDENCE_DIR/process.docx"
cp "$TMP/formal.pdf" "$EVIDENCE_DIR/formal.pdf"
printf '%s  process.docx\n%s  formal.pdf\n' "$DOCX_SHA" "$PDF_SHA" >"$EVIDENCE_DIR/output-sha256.txt"
printf '%s  tender.docx\n%s  tender.pdf\n' "$DOCX_FIXTURE_SHA" "$PDF_FIXTURE_SHA" >"$EVIDENCE_DIR/fixture-sha256.txt"

echo "bidding V1 runtime acceptance passed"
echo "project=$PROJECT_ID pdf_document=$PDF_DOCUMENT_ID docx_document=$DOCX_DOCUMENT_ID quote_snapshot=$QUOTE_SNAPSHOT_ID"
echo "docx=$DRAFT_OUTPUT_ID sha256=$DOCX_SHA"
echo "pdf=$PDF_OUTPUT_ID sha256=$PDF_SHA"
echo "evidence=$EVIDENCE_DIR/evidence.json"
