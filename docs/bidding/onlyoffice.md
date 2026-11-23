# ONLYOFFICE：编辑、保存与同版导出

编制合同见 [Python 招标](../design/python-tender-phase1/README.md)。本页只维护已有 DOCX 的编辑、保存和下载边界。知识库填充属于后续阶段，不作为本轮已实现前置步骤；不设独立正式编制或终稿升级。

## 正文与版本

DOCX 是正文主源。ONLYOFFICE 打开指定已保存版本，用户自由编辑；保存回调持久化后更新 current。不能只以浏览器已发送保存命令视为保存成功。

API 与编辑器通过版本、会话和用途绑定的授权访问文件与回调。保留过期会话、重复／乱序回调和并发保存的校验。实现在 `crates/api/src/tender_editor.rs`（会话、回调、保存）和 `tender_render.rs`（转换），数据在 `tender_editor_sessions`、`tender_draft_*` 表。字段级协议文档待按代码重写（BUG-252）。

## AI 与用户内容

当前只交付「整稿加人工编辑」：整稿重新生成产出新稿，不合并旧稿，也不覆盖用户已保存的版本。编辑后的 AI 候选入稿已推迟（见 [PRODUCT.md](../../PRODUCT.md)）。接入时必须只读冻结的 `OutlineArtifact`，不得改章节、规定文字或把用户已保存的正文交给模型重写。编辑器里的正文以已保存的 DOCX 为准。生成或填充期间用户若已保存更新版本，不能覆盖那一版。

不把编译中间结构当成另一份正文主源。

## PDF 与报告

V3 出件页的每个已保存版本可独立下载 DOCX 或 PDF。PDF 使用 `POST /api/v3/tender-projects/{project}/docx/versions/{version}/pdf`，请求体仅含 `docx_sha256`。项目和版本权限沿用 DOCX 下载。请求摘要须与版本记录一致，实际 DOCX 字节须与记录的长度和摘要一致，身份不一致返回冲突。并发保存新稿时，本次出件仍读取请求指定的版本。

转换沿用 ONLYOFFICE 的并发配额、文件上限、超时和下载地址校验。取源签名绑定项目、所有者、版本、DOCX 摘要和本次转换标识，在转换完成或超时时失效。成功返回 `application/pdf`，文件名标识版本，响应头 `X-KB-Render-Source-Version`、`X-KB-Render-Source-Sha256` 和 `X-KB-Render-Pdf-Sha256` 标明源版本和两份文件的摘要。工作台按版本行发起请求并核对返回的源身份，失败时可从该行重试。报告只包含实际执行的检查和已知问题。

## 部署与验证

保留现有服务 origin、CommandService origin、API origin、JWT／用途签名、字体及许可配置。实际镜像版本以当前部署配置为准，不使用历史方案中的旧 pin。

浏览器通过 `SERVER_ORIGIN` 加载 DocsAPI；API 通过 `COMMAND_ORIGIN` 调 CommandService；文件和回调通过 `API_ORIGIN` 到达 API。密钥不放前端。

健康接口和示例页只证明服务可达，不是保存、保真或许可的证据。必须实际验证：

1. 真实 DOCX 打开、编辑、forcesave、回调落库、关闭重开和下载。
2. 图片、表格、标题、页眉页脚和样式在普通编辑保存后保留。
3. 同一 DOCX 转 PDF，源摘要和版本绑定正确；后续编辑不会让旧 PDF 冒充当前版。
4. 重复、乱序和失败回调、过期会话、并发保存都不覆盖新稿。

历史环境和许可记录不能当成当前生产验收。字体、许可、最终模板外观和完整故障验收仍需落实。
