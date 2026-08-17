# 07 — DocReader 进程

**What to build:** Python 解析服务独立进程能对样例文件产出 Markdown 和逐张图，走 gRPC `ReadStream`。

**Blocked by:** 02 — Review：仓库骨架

**Status:** done

- [x] 代码与 proto 位于 `services/docreader`（proto 为唯一真源，无 *.pb.go）
- [x] `ReadStream` 契约随 brain 拷贝：先 meta 再逐图；空内容带 error
- [x] 引擎 builtin / markitdown / opendataloader；docx+OLE 逻辑在拷贝源里
- [x] 该进程不分块、不 OCR、不写对象存储、不写业务库（源码职责未改）
- [x] brain `docreader` 原测试套件在本树可跑通（缺 `rag_test` 二进制 fixture 时 skip；对照仓同样没有这些文件）
- [x] Rust `docparser` 用 tonic 调 `ReadStream`；`Unimplemented` 回退 unary `Read`；无 `DOCREADER_ADDR` 立即 failed

## Comments

- reality: pytest 98 passed / 13 skipped。tonic 客户端 + mock stream/unary 单测通过。worker 有 addr 才拨号；无 addr 文案与 brain 一致。
- review: 见 08。
