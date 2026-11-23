# 投标台

冷灰仪器盘（见 [DESIGN.md](../DESIGN.md)），栈为 Vite + React 19 + Astryx。用户入口为 **文件 → 编制 → 导出**（左栏当前标下的三步，不是顶栏 Stepper）。编制目标是 ONLYOFFICE Docs 直接编辑真实 DOCX；标题和结构以 DOCX 为准，不由外部树强制回写。

[产品](../PRODUCT.md)、[外壳视觉](../DESIGN.md)、[业务入口](../docs/bidding/README.md)、[招标工作流](../docs/design/python-tender-phase1/README.md)、[编辑/保存](../docs/bidding/onlyoffice.md)。用户在解析完成后点击生成大纲，不因解析就绪自动启动任务。文件 / 编制 / 导出三步入口不变。

当前编制入口使用 ONLYOFFICE DOCX 编辑会话和显式新轮发布，旧大纲/Tiptap 编辑链已删除。保存状态以服务端确认为准；完整冻结招标输入驱动的自动模板生成与同稿 PDF 出件仍按接入计划验收。`npm run dev`/`build` 只构建 SPA。全栈由 `deploy/deploy.sh create` 启动。本地浏览器入口是 **HTTPS**（Caddy 自签，见 [deploy/README.md](../deploy/README.md)）：`https://127.0.0.1:${API_HOST_PORT}/` 或 `https://<本机IPv4>:${API_HOST_PORT}/`。Document Server 浏览器 origin 为 `https://<同一主机>:${ONLYOFFICE_HOST_PORT}/`（`KB_ONLYOFFICE_*`）。首次需信任自签证书。编辑、保存和出件要求见 [ONLYOFFICE](../docs/bidding/onlyoffice.md)。

```bash
cd web && npm install && npm run dev
# http://127.0.0.1:5174 代理到本机 API；全栈请用 https://127.0.0.1:28080/
npm run build
export KNOWLEDGEBRAIN_WEB_ROOT=/opt/workspace/KnowledgeBrain/web/dist
```
