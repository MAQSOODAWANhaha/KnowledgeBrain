# 工作约定

改代码前先读 [活错题本](LESSONS.md) 与 [工程实现与验收标准](docs/code-standards.md)。文档放在哪、哪一页说了算，见 [docs/README.md](docs/README.md)。

## 1. 编码前思考

不要假设，不要藏着困惑。不确定就问。有两种读法时都说出来。有更简单的做法就提出来。看不清就停。

## 2. 文档先行

设计、文档和代码必须一致。三者对不上就是缺陷。

1. 改协议、流程、字段、分包、Prompt、接口、环境变量、端口或部署方式之前，先改 [docs/README.md](docs/README.md) 对应的权威页，再改代码。同一次提交里同时带上文档和代码。
2. 只修实现、不改设计的缺陷，可以不改设计页，但问题记录要写。
3. 发现文档和代码不一致：先查清哪边对。代码对就改文档；文档对就记缺陷再改代码。不能两边各留一种说法。
4. **只写现在时，禁打补丁**：文档只写现状和已经定下的设计，修改时直接覆写或删除旧句，禁止留「若旧句…以本句为准」「本轮尚未…」。计划写在 `plans/`，做完或放弃就删，结论并入权威页。过期内容直接删除，不建 archive，Git 历史够用。
5. 一件事只在一页写。别处只放链接，不复制规则、公式、命令或字段。文档中引用的源码路径、环境变量、端口必须与代码核对一致，并通过 `python3 scripts/ci_check_docs.py`。
6. 运行结果（run id、Token、镜像号、套件次数）只写交接，不写进设计页和问题记录。

## 3. 简洁优先

只解决要求本身。不为一次性代码做抽象，不加没人要的配置，也不为不可能发生的情况写错误处理。资深工程师会觉得多余，就删。

## 4. 精准修改

只改必须改的。匹配现有写法。不要顺手重构旁边的代码。修缺陷前全仓 `rg` 同类兄弟点一次修齐。自己改出来的无用导入和函数要删；原本就在的死代码，除非被要求，否则只指出来。

## 5. 目标驱动与可审计执行

- 先写能判断对错的标准，再改到通过。修缺陷必须有「回退改动会真实失败」的 Red→Green 反证测试。
- 严格区分 L0 静态守卫 / L1 离线单测 / L2 隔离集成与 E2E / L3 真实业务验收（命令矩阵见 [docs/code-standards.md](docs/code-standards.md)），禁止互相替代，禁止以 `main` 上本来就红为由跳过。
- 多会话并行时使用独立 git worktree；单 commit 单目标；每次 commit 后核对 `git show --stat HEAD`，并在 Commit Body 附上 `Verification:`（Red→Green / Ran / Not run）。

## Python 招标问题记录

Python 招标分析中每次发现问题，先追加或更新
[问题与修复记录](docs/design/python-tender-phase1/bug-fix.md)，记录现象、影响、根因和解决方案；
根因未确认时明确标注待查。修复后补齐实际测试、部署和真实文件验证状态，不以离线通过代替真实流程完成。

修改招标分析的协议、流程、分包或 Prompt 前先回答下面五个问题；**最终生效的规则与调用公式**更新到 [业务工作流](docs/design/python-tender-phase1/business-workflow.md) 对应章节，**逐条五问答辩**写入 Commit Body / PR 或 `plans/`，不把历史答辩日志永久堆在权威设计页：

1. 全文件调用次数的公式，以及随文件规模如何增长；
2. 单个请求最坏情况下的输入和输出；
3. 会新增哪些问题，各由谁通过什么出口解决（不能只有阻断没有出口）；
4. 修订一次需要重跑多少调用；
5. 是否要求模型誊写宿主已知或可推出的内容。

运行证据和回放素材放在仓库外的 `/opt/workspace/kb-tender-evidence/`，不放 `/tmp`；
不提交招标原文、完整请求或密钥。动态运行状态只在 [交接文档](docs/design/python-tender-phase1/handoff.md) 维护。

## Cloud Agent

每次启动会拉起本机开发栈。宿主进程不要使用 Compose 网络里的服务名。

- Postgres `127.0.0.1:15432`，Redis `127.0.0.1:16379`，对象存储 `127.0.0.1:19000`。`minio/minio` 已从 Docker Hub 下架，对象存储使用 Chainguard MinIO，bucket 为 `knowledgebrain`。
- API 是 `http://127.0.0.1:8080`（`/ready`）。网页是 `http://127.0.0.1:5174`，并代理到该 API。
- Chat 模型与 `KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS` 必须显式配置；默认招标模式不要求 Embedding，启用知识库后才检查相关模型。
- 未配置 LDAP 时使用本地密码登录；首次启动必须显式配置 `KB_ADMIN_USERNAME`、`KB_ADMIN_PASSWORD` 和 `JWT_SECRET`。
