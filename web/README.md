# 投标台

Apple / iCloud 工作台。打开标是一张编制台：文件 → 编制 → 导出。编制步是 Word 式三栏（左大纲树 / 中 Tiptap 连续画布 / 右知识提示），不是 ①～⑥，也不是 Markdown 源码页。

产品与交互：[`../PRODUCT.md`](../PRODUCT.md)、[`../DESIGN.md`](../DESIGN.md)、[`../docs/bidding/authoring.md`](../docs/bidding/authoring.md) §2.4。落地：[`../plans/bidding/frontend-authoring.md`](../plans/bidding/frontend-authoring.md)。

```bash
cd web && npm install && npm run dev
# http://127.0.0.1:5174  代理到 :8080

npm run build
export KNOWLEDGEBRAIN_WEB_ROOT=/opt/workspace/code/KnowledgeBrain/web/dist
```
