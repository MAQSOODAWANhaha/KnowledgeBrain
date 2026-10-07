# ONLYOFFICE 技术接入与验收

生成合同见 [大纲运行时](../../docs/bidding/outline.md)。本页只保留编辑器技术约束，不维护独立 O 阶段账本或旧草稿／终稿流程。

## 接入职责

- 浏览器通过 SERVER_ORIGIN 加载 DocsAPI；API 通过 COMMAND_ORIGIN 调 CommandService；文件和回调通过 API_ORIGIN 到达 API。
- origin、JWT、用途签名、文件授权和回调来源检查沿用现有实现；不将密钥放入前端。
- ONLYOFFICE 打开指定版本，保存以回调持久化为准，重复／乱序／旧会话不得覆盖 current。
- DOCX 是正文主源，PDF 从同一保存版本转换；不得维持另一份竞争正文或后台双向同步。

完整字段和协议见 [docx-editor.md](../../docs/bidding/docx-editor.md)，产品目标见 [onlyoffice.md](../../docs/bidding/onlyoffice.md)。镜像、端口和环境变量以现行 Compose 与部署文档为准。

## 与填充的接缝

知识库填充读冻结大纲，不改章节和规定文字。编辑器仍以已保存 DOCX 为正文。版本冲突不覆盖，成功后编辑器和下载刷新 current。旧的整稿兼容性白名单不是大纲发布门，记录在 [archive](../../docs/bidding/archive/docx-composition.md)。

## 必须的实际验证

1. 真实 DOCX 打开、编辑、forcesave、回调落库、关闭重开和下载。
2. 图片、表格、标题、页眉页脚和样式在普通编辑保存过程中保留。
3. 对允许填充的文档验证现有内容保持；对不支持内容验证提交前阻止。
4. 同一 DOCX 转 PDF，源摘要／版本绑定正确；后续编辑不会把旧 PDF 冒充当前版。
5. 重复、乱序和失败回调、过期会话、并发保存不覆盖新稿。

健康接口与示例页不是保存重开、保真或生产许可证据。字体、部署授权和许可按实际环境核验。历史结果在 [archive](../../docs/bidding/archive/README.md)，只保留当时的事实。
