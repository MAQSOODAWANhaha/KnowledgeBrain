# R1 真实隔离启动与对象生命周期

> 历史验证记录：仅说明所记录版本的结果，不是现行设计或新流程验收。当前方案与任务见 [统一方案](../../plans/bidding/product-two-phase.md)。

修复发布入口的依赖环境校验后，已用实际生产 Dockerfile 构建的镜像完成全新隔离环境验收。最终项目为 `kb-release-7dj2b332`，原始记录在 `/tmp/kb-release-final-7dj2b332/`，可归档副本及 SHA256 清单见 [`artifacts/release-live-acceptance/`](../../artifacts/release-live-acceptance/)。这是当前工作树的隔离运行证据；干净已提交候选和 hosted 发布 gate 尚未具备。

## 验证结果

| 验证 | 实际结果与证据 |
| --- | --- |
| 完整启动 | `knowledgebrain-release` exit 0，9 个服务按实际 Compose 启动，migrator exit 0，API/Worker/Retention 和依赖健康；`final/release-start.stdout` |
| 启动先后 | Docker 实际事件显示三个 runtime 的 start 均晚于 migrator exit 0；`final/startup-order-verification.json` |
| 镜像身份 | 四个 Rust 角色使用实际 runtime RepoDigest，DocReader 使用实际 parser RepoDigest；发布工具执行 inspect 校验，快照保存容器 image config ID/full ref；`final/replay-verification.json` |
| 重复执行 | 第二次入口 exit 0；9 个容器的 ID、image、启动/退出时间、重启次数和整份 schema receipt 前后完全相同，migrator 未重跑；`final/replay-verification.json` |
| 空对象卷上传/读取 | 起始对象卷无文件；随机合成文本经真实 API 上传到两个版本，产生两个不同文档和两个业务引用。两版本下载与原始 bytes 完全相同，独立签名 GET 核对 MinIO bytes；`final/object-verification.json` |
| 部分引用释放 | 经 API 删除第一个文档，实际 Worker 将 owner 数降到 1；第二版本、本地及 MinIO bytes 仍可读，没有删除 tombstone |
| 最后引用释放 | 经 API 删除第二个文档，由实际 Worker/Retention 处理。registry 为 deleted、owner 为 0、tombstone 为 1；本地文件不存在、MinIO GET 404、版本下载 404 |
| 删除凭据 | 实际联查 registry、deletion artifact 与 tombstone，确认同一摘要和字节长度、删除时间及 `system:retention-consumer`；匹配记录恰为 1；`final/final-verification.json` |
| 最终健康与 receipt | 对象生命周期后，schema receipt 未变，三个 runtime 的生产健康命令均成功；`final/final-verification.json` |
| 清理 | 按预先核验的 project/acceptance 标签和 ID 停止并删除本轮 9 个服务容器、5 个卷、专属网络及 registry；owned runtime 残留 0，原有容器 ID 全部仍在；`final/cleanup-verification.json` |

`final/stage-exits.json` 中准备/启动、重放、对象生命周期、最终核对/清理四个执行阶段均 exit 0。运行中的业务断言没有 ignored、skip 或测试替身消费者。

## 输入与适用范围

- 继续读取当时实际 `deploy/.env` 的模型配置，未临时指定模型；隔离资源密码、namespace、数据库及 bucket 单独生成。原始 env 和 resolved Compose 含凭据，不归档。
- 应用 Docker 网络为 internal，阻断外部出口。Docker 在此配置下没有建立宿主端口映射，验收通过容器内真实 TCP 访问 API；没有为测试开放外部出口。镜像仅来自本机 loopback registry 和已固定 digest 的依赖。
- 对象样本为本轮随机生成的 56 字节文本；测试直接创建所属空库中的 workspace/product/version 前置夹具，存储验证所用版本关闭索引和内容生成。上传、读取、文档删除和实际异步回收均走产品实现。该测试不验证知识提取、真实模型、登录流程或招标内容质量。
- runtime 镜像为 `sha256:60560ffca56c65046714e385d693eb63dbfef72888313ab54cfd0d7fb7883c01`，DocReader 镜像为 `sha256:d4c036a48b1c6afb146de84e93f113f126d1de285beb34f60b34d02af8588de3`。实际 full RepoDigest 和宿主发布 binary 摘要均记录在 descriptor/manifest；构建来源见前批 `artifacts/workspace-quality/release/source-manifest-attempt2.json`。重复使用这批镜像不代表重新构建了当前工作树所有变动。
- 归档的 Python 文件是本轮执行脚本快照，引用本机隔离路径；原始私密 env/Compose 与脚本快照不能作为生产发布配置。需要重做时须先创建新的专属环境，再执行这些验证阶段。

## 保留的失败

最早启动因发布工具错误要求 Redis 显式 environment 而失败，修复及原证据见[发布入口记录](release-entry.md)。第一次修复后重跑 `kb-release-9rl_lzca` 的启动、重放、对象业务断言均通过；补充凭据核对的验收 SQL 错把 `object_deletion_artifacts.id` 写成 `deletion_id`，最终核对 exit 1。清理仍成功且残留为 0。源码复核同时修正了验收脚本对 `CMD`/`CMD-SHELL` 健康命令的处理，并在上述全新项目完整重跑通过。第一次重跑的记录保留在 `retry-1/`，不能用其 cleanup 通过掩盖最终核对失败。

R1 的隔离启动及对象生命周期缺口现已取得真实证据。R1 正式完成仍需干净发布候选与对应构建身份的验收；R3 named required jobs/hosted gate 另行落实。本结果不改变 O1 完整 Agent 样稿及同版本 PDF 尚未验收的状态，也不授权生产部署或现有 namespace reset。
