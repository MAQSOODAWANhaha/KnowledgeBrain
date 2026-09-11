# ONLYOFFICE 保存生命周期实测

本轮在独立 Community 9.4.0-129 / Chromium 151.0.7922.34 上完成真实 DOCX 的两次 forcesave、继续编辑后的最终保存、新 key 重开、旧回调显式重投，以及无修改关闭后的同 key 重开。**这是协议与文件内容实证，不是 KnowledgeBrain 会话/版本发布已接入；本轮没有新增或修改数据库结构。**

## 验证方法与结果

使用用户提供的真实 DOCX 副本，随机生成编辑标记、文档 key、用户及 `userdata`。浏览器通过真实画布和键盘编辑，采集器核对回调 HMAC 签名和签名载荷一致性，读取该专属 Document Server 的实际 DOCX，检查摘要与正文标记。

| 场景 | 观察与断言 |
| --- | --- |
| 两标签页打开同一 key | 编辑器均进入 ready；真实回调包含同一 key 的加入/退出通知。此 key 由试验输入指定，不是产品并发分配测试。 |
| 第一次 forcesave | 命令 `error=0` 后收到 `status=6 / forcesavetype=0`，`userdata` 匹配本次请求；下载文件包含第一个随机标记。 |
| 第二次 forcesave | 同一 key、不同 `userdata`；下载文件同时包含两个标记，各一次，摘要与第一次不同。 |
| 再编辑后关闭 | `status=2` 最终文件包含全部三个标记，各一次；第三个标记不在上一次 forcesave 文件中。最终回调未携带 `userdata`。 |
| 旧回调显式重投 | 最终保存后，采集器将捕获的第一次 forcesave 请求连同原签名再次投递；签名核对通过，仍下载到第一次文件的相同摘要。**这是试验主动重投，不是 Document Server 自发重试或自然乱序证明。** |
| 最终文件以新 key 重开 | 编辑器 ready；截图可见三个编辑标记。新 key 由试验明确生成，不代表产品已实现 key 轮换。 |
| 无修改关闭再打开 | 同一 key 实际收到 `status=4 → status=1`，重开后仍能编辑模式加载。此场景是正常关闭重开，未模拟断网。 |
| 原件与清理 | 原稿摘要不变，专属容器、卷和网络删除，临时密钥文件移除，清理 exit 0。未访问现有 API/worker/retention 或数据库。 |

下载摘要（本次运行的证据值，不是生产常量）：

| 文件 | SHA-256 |
| --- | --- |
| 第一次 forcesave / 其重投 | `0d1ef4a26725ef9b21768b51bb295d2d444d37f9d008b46b55a390ca3e481281` |
| 第二次 forcesave | `554785ed101adf03ef253ed195dfde2ab8d9da5d7fcefa92875d043c62f70216` |
| 最终保存 | `46a170010cea34d36dfeacd18af87a745110d95e40ba527127a47e5096e40014` |

本轮没有复测全文/所有表格保真、PDF 或字体；相关已知边界见 [O0 结果](onlyoffice-o0-results.md)。原稿来自 `testdata/bid/60adc38f81464a0a93898eb435b506d6.docx`，SHA-256 为 `4a7c68d8c20688a12f7e978e97abbbd5b2cec116728ad3f3f639558c8091b47d`。生产许可、字体与最终模板外观仍待落实。

## 对最小实现的约束

1. 活动会话 key 与已保存文件版本必须分开：两次真实 forcesave 已证明可以在同一 key 下得到不同内容的文件。不能把每次保存后的摘要直接作为活动 key。
2. 首个接入切片可围绕后端发起、带 `userdata` 的 forcesave 建立关联；复用现有请求幂等和 CAS。`userdata` 识别请求，不是全局递增版本号；无关联的其他 forcesave 来源不能按到达时间覆盖当前文件。
3. `status=2` 需单独作为最终保存处理：它在本次没有 `userdata`，不能被强制套进 forcesave 请求格式。最终保存必须先验证并持久化，再终结该活动 key；迟到 forcesave 不得回退最终版本。
4. `status=4` 不创建文件版本，也不能自动视为永久关闭并立即旋转 key。实际同 key 的 `4 → 1` 与官方重连描述一致，但断网条件仍需独立验证。
5. 签名只证明可信发送方及内容未被篡改。本次原签名旧文件重投是直接反例：还需校验轮次、活动 key、保存关联和终结状态；通用 CAS 自身也不提供文件新旧顺序。
6. 因此持久化需求应先收敛为活动 key、打开基线、保存关联/终结状态与现有 current/version 的绑定。**目前没有实证要求新增独立会话表**；下一实现先评估在现有 DOCX current 中维护活动关联，并复用版本、幂等和 audit，待实际 writer/reader 与事务约束明确后再决定必要字段。

仍待实现/验证：KnowledgeBrain 的签名配置和受控文件读取、生产 JWT 时效与用途/越权校验、回调下载安全边界、真实存储失败及重复/乱序不回退、最终保存与重新加入竞争、API 重启/并发分配，以及断网后的恢复。试验采集器没有产品 current pointer，只记录收到的每个文件，不能把它的接受结果当作产品幂等或迟到保存保护验收。

## 可复跑工具与证据

[`scripts/onlyoffice_lifecycle_probe.py`](../../scripts/onlyoffice_lifecycle_probe.py) 是显式运行的隔离试验工具，需要本机已安装 Python Playwright、浏览器文件以及已缓存、带 digest 的 Document Server 镜像。所有样稿/镜像/浏览器路径由参数传入；key/请求标记/专属资源标签/回环端口动态生成。不会拉取镜像，不读取产品数据库配置，也不自动修改部署配置。

```sh
python scripts/onlyoffice_lifecycle_probe.py \
  --image "$ONLYOFFICE_TEST_IMAGE" \
  --sample "$ONLYOFFICE_TEST_DOCX" \
  --browser "$PLAYWRIGHT_TEST_BROWSER" \
  --evidence-dir "$ONLYOFFICE_TEST_EVIDENCE"
```

默认每步超时 180 秒、下载上限 64 MiB，可通过 `--timeout` / `--max-bytes` 调整；这是试验预算。采集器在专属容器内运行，两个服务端口只映射宿主回环地址。该容器为读取同容器试验样稿而启用 private-address 访问；回调下载只接受这个服务的已知 origin 和 cache 路径并拒绝重定向。此配置及本机 origin 转译仅供试验，不能搬作生产 SSRF 策略。采集器不是完整生产 JWT 验证器。

本机工作包 `/tmp/knowledgebrain-onlyoffice-lifecycle.d0s167kq/`：

- `run-21e7579b1bc546ee9bfcd197f97b3685/`：通过的 `result.json`、`events.json`、真实保存 DOCX、`reopened.png`、镜像/源码副本及 `run.exit=0` / `cleanup.exit=0`。
- `run-d2f739c211b8477cab09502228a1d378/`：首次画布点击被可见 overlay 拦截，未执行保存场景；失败源码、事件和清理收据保留。修正为点击实际可见 overlay 后通过，没有放宽文件内容断言。
- `attempt2.log` 保存最终执行日志。最终脚本相对执行副本仅删除两个未使用 import 并纠正“正常重开不等于断网”的注释，执行逻辑相同；证据目录是本机临时位置，不是永久发布资产。
