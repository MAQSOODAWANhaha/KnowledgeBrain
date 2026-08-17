# 08 — Review：DocReader 对照 brain

**What to build:** 对照 brain DocReader 协议与职责边界，确认没有改成 HTTP 或把分块塞回来。

**Blocked by:** 07 — DocReader 进程

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] 协议文件仅为 gRPC proto
- [x] 职责声明与 brain 一致：只出 Markdown + ImageRef
- [x] 本仓库独立验证引擎回退与 OLE 特例（`tests/test_parser_routing.py` 3 项通过）
- [x] 偏差已记明（rag_test 二进制 fixture 对照仓也没有；TLS/token 拨号尚未接）

## Comments

- review 2026-08-15: 协议仍是 gRPC proto，无 HTTP 解析面。OLE magic `D0CF11E0` 重路由已用原测试核实。
- review 2026-08-15e: `docparser` tonic `ReadStream`（先 meta 再图）+ unary 回退有单测。无 `DOCREADER_ADDR` → 立即 failed。TLS/mTLS/GRPC_AUTH_TOKEN 客户端未接。

