export function DraftReady() {
  return (
    <section className="card stack" data-testid="draft-ready">
      <h2 className="h3">草稿模板</h2>
      <p className="text-sm">分析已发布草稿。终稿需要另一次独立复核请求，这里不会启动编制 Agent。</p>
      <p className="text-sm">工作区还没有可编辑稿件。编译写入完成后可直接打开编辑器。</p>
    </section>
  );
}
