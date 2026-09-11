# 发布入口实现与隔离验证

本轮落实 [R1](../../plans/implementation-tasks.md) 的宿主发布入口，复用 `platform::ReleaseDescriptorV1`、平台 identity 校验和 JCS 摘要。没有新增 migration、表或依赖库，仅为已有 Tokio 启用 process feature。最初完成测试替身验证，后续已实际构建镜像并尝试专属隔离启动；没有操作业务库或部署生产。

## 已实现的边界

- 生产发布入口是宿主 `knowledgebrain-release`。descriptor、env file、project 必填；本地 `--check-only` 不调用 Docker。descriptor 拒绝未知/重复字段；env 拒绝重复 key，必须显式给出 canonical namespace；schema 必须与 binary 的内嵌合同相同。descriptor、env、Compose 和 schema bytes 在执行前后及每次启动前复核。
- 校验复用现有 typed validator；用已有 dev JSONSchema validator 对正常/非法字段和镜像引用做差分回归，未引入第二份发布语法。该回归发现并修复共享校验器原先接受 `registry:12:34/path` 的问题。额外的安全拒绝（如重复 JSON 字段）由 typed parser 承担。
- Docker 子进程只继承访问本机 Engine 必要的宿主变量，再载入显式 env file。应用变量不会从旧 shell 隐式覆盖 Compose 插值。命令参数不拼接 shell；错误不输出可能包含密码的 Engine stdout/stderr；超时终止命令。
- 实际解析 Compose 后，全部服务使用显式 project 容器名及本次 attempt 标签。临时 override 的目录/文件权限分别为 0700/0600；文件 fsync，成功前显式核对删除结果。四个 Rust 组件注入 exact identity、角色 `BIN` 与原 descriptor 的唯一只读挂载，五个组件都只用 descriptor 的 digest image。依赖镜像也必须 digest pinned。
- 先拒绝已有服务的 scope/image/env/mount/health 不一致，再 pull、检查 full RepoDigest。实际容器 image config ID 与 inspect 得到的 ID 对应，同时核对 `.Config.Image` full ref；manifest digest 不充当 config ID。Rust/DocReader 的入口和命令须与镜像配置一致，健康探针须与解析后的 Compose 配置一致；通过 inspect 后再运行该探针。
- 先等待数据面及 DocReader，再执行 migrator。migrator 必须 exit 0、身份/环境/挂载/命令符合，才能启动 API/Worker/Retention。runtime `/ready` 复用已有 receipt gate。重复执行对已完成的同一组服务只读检查，不重新运行 migrator。
- 失败只停止本次 attempt 且 project 匹配的容器，不停止原有服务、不删除容器或数据卷。清理失败也返回失败；失败留下的停止容器不会被下次隐式替换。旧 `docker-compose.pro.yml` 和示例 env 中的 tag 发布参数已经删除。

## 首批入口证据

证据位于 `artifacts/release-entry/`，均使用合成配置，没有真实招标文本或部署凭据。

| 验证 | 结果与范围 |
| --- | --- |
| 发布入口 Rust 测试 | 14/14，包含新启动/只读重放、迁移与入口失败拦截、已有组件/依赖漂移、伪造探针、full RepoDigest、env/mount、输入漂移、临时文件权限与清理、超时/错误脱敏及 schema 差分 |
| 平台现有 release 合同 | 6/6，沿用现有 hash fixture、角色绑定及 runtime startup 合同 |
| Clippy | 宿主发布 binary `-D warnings` 通过 |
| 宿主 binary 与 wrapper | build 通过，wrapper `--check-only` 不调用 Docker，输出 `images_verified=false`、`deployed=false` |
| 真实 Compose 合并 | Compose 2.40.3，两个实际 `config --format json` 调用；9 服务隔离名称、五镜像、四角色/摘要/只读挂载均通过，显式 env 中的模型值优先于旧 shell 值 |
| Engine 隔离 | 其余 Engine 调用全部由测试替身截获；第一次 pull 被故意拒绝，入口非零退出，无实际 image pull、Engine 查询、up/stop 或数据操作；两个临时 override 均已删除 |

复现命令（使用仓库规定 Rust 工具链）：

```bash
cargo test --locked -p platform --bin knowledgebrain-release
cargo test --locked -p platform --lib release::tests
cargo clippy --locked -p platform --bin knowledgebrain-release -- -D warnings
cargo build --locked -p platform --bin knowledgebrain-release
python3 artifacts/release-entry/verify_compose.py --binary target/debug/knowledgebrain-release
```

本机实际使用 `--offline --target-dir /tmp/kb-retire-target`。具体日志、binary/source SHA 和结果见 `verification.json`、`compose-verification.json`、`compose-calls.json`。`compose-resolved-synthetic.json` 仅保存合成配置；不得将真实 Compose 输出不经处理归档。

## 真实构建、启动与后续验收

后续使用实际生产 Dockerfile 完成 Rust runtime（包含四个角色 binary）及 DocReader 镜像构建，并仅推入本机临时 loopback registry，取得真实 RepoDigest。Node 22 构建暴露的 npm lockfile 缺项已修复，Rust release 构建已启用 `--locked`。此证据来自带继承修改的工作树及源码 manifest，不是干净已提交发布候选。

首次完整隔离启动返回 `RELEASE_NOT_READY: resolved environment missing`（exit 1）。实际 Redis Compose 没有显式环境映射，发布工具误将其当成配置缺失；已修复为核对声明的映射，继续独立强制 Rust identity/BIN 校验。新增依赖无显式环境的启动/重放回归后，发布入口测试现为 **15/15**；全工作区 check、Clippy 和默认测试通过，见[质量检查记录](workspace-quality-results.md)。原失败保留，后续已在新隔离环境完成真实启动重跑，见下文。

原始构建及启动证据保留于 `/tmp/kb-release-live-564icf9k/`；其中 `isolated.env` 和 `compose.json` 含运行凭据，不得原样输出或归档。应用使用 internal 网络阻断外部出口，未执行真实模型调用。本次 9 个容器、5 个卷、专属网络及本机 registry 已按登记身份清理，`cleanup-verification.json` 确认 runtime 资源残留为 0；本地测试镜像仍保留。脱敏可归档证据见 `artifacts/workspace-quality/release/`。

首次 Rust 构建请求曾因自动审批 429 限流未执行；后续前置审计将普通 parser unit fixture 误判为真实样稿而失败，但重试说明错误地声称审计成功。获批重试实际执行后因 npm lockfile 失败。补核源码 manifest 确认无 env 或真实招标文件，并保留该失误及审计记录 `rust-build-authorization-audit.json`；不覆盖原失败。

后续专属项目 `kb-release-7dj2b332` 已完整通过真实启动、migrator 先于 runtime 的事件核对、全服务 readiness、整 receipt 与容器身份不变的重复执行，以及空对象卷的真实 API 上传/读取、多引用保护、实际 Worker/Retention 最终回收和精确删除凭据核对。9 个服务容器、5 个卷、专属网络及 registry 清理后零 runtime 残留。详细步骤、第一次重跑中验收脚本失败及最终四阶段 exit 0 证据见[真实隔离验收](release-live-results.md)。没有用业务数据库做验收。

R1 仍未正式完成：上述镜像绑定带继承修改的工作树及源码 manifest，还需绑定干净候选 SHA、Cargo.lock 与对应五组件实际 RepoDigest 完成候选验收。R3 的 named required jobs/hosted gate 另行跟踪，不能将本机通过写成生产放行。

本轮不改变 O1/O2 顺序，不证明真实招标 Agent 整稿或同版本 PDF 已生成。真实样稿的提取拒绝项和外部模型授权仍见[样稿结果](full-sample-results.md)及[逐页验收](real-tender-acceptance-review.md)。
