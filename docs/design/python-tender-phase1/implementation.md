# 实施清单与当前事实

## 文档阶段

- [x] 核查 AGENTS.md、工作区及现有 Python 目录/依赖/默认入口。
- [x] fetch origin/main，基线 `e8b681a0997760ec3f567223fe7eb82b622f5ed7`，工作区干净。
- [x] 提交方案、职责边界、独立/未来接口合同、实施及验收清单。
- [x] 创建 [Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)。
  文档 commit `5a77c7d2f7c36acfd0d7c64c1073ca776e1eb734`；分支 `feat/python-tender-phase1`。

## 实施（严格先骨架验收，再领域）

- [x] 锁定 Python 3.11 依赖、setuptools 完整包表、真实服务连接。
- [x] 真实业务表/文件产物适配、命令领取/回执/补投、租约/围栏、取消。
- [x] LangGraph + PostgreSQL checkpoint + Dramatiq AsyncIO 骨架与暂停/恢复。
- [x] 先完成 A 类可靠性骨架首轮测试并记录结果（4 执行 + 12 持久测试）。
- [x] 来源登记、原生解析/OCR、覆盖问题、冻结/分包与发送视图（真实模型OCR语义另待D）。
- [x] 模型配置/请求/usage/uncertain；并行要求提取与局部失败隔离。
- [x] 全来源独立验证、规则反查、确认版本摘要与响应计划。
- [x] 独立 CLI 提交/状态/确认/取消/改版与 Python 专用启动配置。
- [x] B/C 类独立验收、打包/镜像证据已记录；PR 同步进展。
- [ ] D 类真实 LLM 语义验收（与 mock 分开，付费前确认）。

## 明确不在本期完成

- Rust 接入、outbox/API/事务变更、切流、删除旧实现、合并和部署。
- knowledge 检索、正文生成、成稿审查、最终导出。

## 已知基线与风险

- 前轮 main CI 记录：Rust bidding 500 passed / 2 failed / 24 ignored；本次不修 Rust。
- Python 镜像 3.11；本机默认 3.12，需另建 3.11 验证环境。
- 现有 setuptools 显式列举包，新增包及提示资源遗漏会导致镜像缺模块。
- 已只读检查 CHAT/VLM：endpoint `https://cpamc.818996.xyz/v1`，model `gpt-6-luna`，
  凭证存在，但 CHAT 上下文容量未配置；Library/共享/testdata 没有真实 PDF/Office/图片原件。
  尚无付费全流程调用授权，不发送文档；合成样本与 mock 不计为真实语义验收。

## 偏差审查记录

1. 原 parser 的 PDF 图筛选/Excel 行简化是新完整性合同与旧接口用途不同；在新证据适配器补齐，
   不改旧 parser/Rust。先更新 domain-contract；真实多格式测试通过。
2. 全局 freeze 回执不足以恢复局部成功 OCR/文件；属于设计粒度细化。先补文档，按文档/图片/
   包持久回执，再汇总索引；局部失败恢复测试通过。
3. 独立审阅发现 dispatcher 掉库退出、发布失败无限重投及初始化漏出错误保护，属实现问题。
   合同明确恢复与分类后修复；真实 PG 拒连恢复、发布/执行共享上限和终态回执测试通过。

独立实现与确定性验收完成；真实 LLM 语义验收和未来 Rust 生产集成保持未完成。

## 追加评审（基于 a009307，2026-10-14）

合同先行补齐同来源领域修订/确认失效、恢复配置固定、标注集 ≥95%/否决项100%目标。
实现与真实文件准备继续在 Draft PR #24；本轮测试和真实分包结果待逐项记录。
