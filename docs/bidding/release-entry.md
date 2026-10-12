# 发布入口边界与验收

本页仅说明宿主发布工具，不是投标生成合同；领域入口见[招投标文档](README.md)。当前候选必须重新验证，旧本机结果不代表生产放行。

## 工具边界

- 生产发布入口是宿主 `knowledgebrain-release`。descriptor、env file、project 必填；本地 `--check-only` 不调用 Docker。descriptor 拒绝未知/重复字段；env 拒绝重复 key；schema 必须与 binary 的内嵌合同相同。descriptor、env、Compose 和 schema bytes 在执行前后及每次启动前复核。
- 校验复用现有 typed validator；用已有 dev JSONSchema validator 对正常/非法字段和镜像引用做差分回归，未引入第二份发布语法。该回归发现并修复共享校验器原先接受 `registry:12:34/path` 的问题。额外的安全拒绝（如重复 JSON 字段）由 typed parser 承担。
- Docker 子进程只继承访问本机 Engine 必要的宿主变量，再载入显式 env file。应用变量不会从旧 shell 隐式覆盖 Compose 插值。命令参数不拼接 shell；错误不输出可能包含密码的 Engine stdout/stderr；超时终止命令。
- 实际解析 Compose 后，全部服务使用显式 project 容器名及本次 attempt 标签。临时 override 的目录/文件权限分别为 0700/0600；文件 fsync，成功前显式核对删除结果。四个 Rust 组件注入 exact identity、角色 `BIN` 与原 descriptor 的唯一只读挂载，五个组件都只用 descriptor 的 digest image。依赖镜像也必须 digest pinned。
- 先拒绝已有服务的 scope/image/env/mount/health 不一致，再 pull、检查 full RepoDigest。实际容器 image config ID 与 inspect 得到的 ID 对应，同时核对 `.Config.Image` full ref；manifest digest 不充当 config ID。Rust/DocReader 的入口和命令须与镜像配置一致，健康探针须与解析后的 Compose 配置一致；通过 inspect 后再运行该探针。
- 先等待数据面及 DocReader，再执行 migrator。migrator 必须 exit 0、身份/环境/挂载/命令符合，才能启动 API/Worker/Retention。runtime `/ready` 检查运行依赖，不执行数据库结构摘要门禁。重复执行对已完成的同一组服务只读检查，不重新运行 migrator。
- 失败只停止本次 attempt 且 project 匹配的容器，不停止原有服务、不删除容器或数据卷。清理失败也返回失败；失败留下的停止容器不会被下次隐式替换。旧 `docker-compose.pro.yml` 和示例 env 中的 tag 发布参数已经删除。

## 当前候选验收门槛

绑定干净候选 SHA、Cargo.lock、真实 Docker build context 和五组件 RepoDigest；本机 Cargo 通过不能代替生产 Dockerfile 构建。Rust builder 必须包含其 include_str! 引用的 发布 descriptor schema 文件。

分别验证 check-only 无 Docker 副作用、Compose 身份/环境/挂载、migrator 先于 runtime、依赖 readiness、同身份只读重放、失败与清理边界、对象引用保护和最终回收。只在专属授权环境运行，不操作业务数据。R1 候选验证、R3 hosted gate、字体许可和 O1/O2 产品验收分别记录，未测不得写通过。历史本机日志不再作为当前入口文档。
