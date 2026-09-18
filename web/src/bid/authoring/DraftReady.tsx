export function DraftReady() {
  return (
    <section className="card stack" data-testid="draft-ready">
      <h2 className="h3">章节骨架</h2>
      <p className="text-sm">章节大纲已生成。骨架 Word 存入工作区后即可打开编辑器，正文可自行编写，也可一键 AI 填充。</p>
      <p className="text-sm">工作区还没有可编辑稿件。请查看生成任务是否存在保存错误。</p>
    </section>
  );
}
