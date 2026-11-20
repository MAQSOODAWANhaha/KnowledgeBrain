# Python 招标交接

动态状态只写在这里。字段看[字段字典](field-dictionary.md)，发送次数看[业务工作流](business-workflow.md)，问题看[问题记录](bug-fix.md)。

## 现在

- 2026-11-20：提交 `7b03204` 的 docreader 镜像 `kb-docreader:w0-7b03204` 已部署到 `kb-literal-candidate`（只换 docreader、worker、dispatcher；API 仍是 `candidate-3e1046f`）。222 新分析运行 `32eb5604-77fd-4b4a-abe3-d49ee19ab922` 用 17 分 41 秒到 `waiting-confirmation`：OCR 3、extract 3、verify 9 次全部是 review，全部成功、无重试，与计划的 12 个请求一致。产出 131 条事项、318 个人工问题，其中 248 个是 review 已覆盖的未处置范围（BUG-261）。Token：extract 180,366，verify 304,230，OCR 11,723。尚未确认，未进入编制。证据在 `/opt/workspace/kb-tender-evidence/2026-11-20-w0-candidate-7b03204/`。
- 四份真实成品（Bidding、安服、111、222）的语义验收仍是 0/4。离线单测不能代替。
- 分析源码里，verify 只发 review。`locate`、`associate`、`joint` 不再发送。缺页和漏报原子由宿主记待核。正在跑的镜像还是旧路径，要重新部署才生效。
- 编制源码里，漏掉的来源原子补成待核并接受该包（BUG-250）。重复引用、未知编号、上下文环和正文与上下文重叠仍然拒收。111 的真实包还没按这个规则重放。
- Word 导出写入前去掉 `U+FFFE` 等 XML 非法字符（BUG-249）。冻结台账不改。真实 Bidding 还没重新导出。
- 证据在仓库外 `/opt/workspace/kb-tender-evidence/`。本文件不抄 run id、Token 和套件次数。

## 已删除的旧日记

2026-11 中旬及更早的运行日记已从本文件删除。那些段落里的「本段优先」、镜像号和互相覆盖的 0/4 快照不能再当当前事实。
