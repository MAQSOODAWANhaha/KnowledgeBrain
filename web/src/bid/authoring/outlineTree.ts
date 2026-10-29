import type { OutlineNode, TenderChapter, TenderTarget } from "../api/workbench";

export function findChapter(tree: TenderChapter[], id: string): TenderChapter | undefined {
  for (const chapter of tree) {
    if (chapter.chapter_id === id) return chapter;
    const found = findChapter(chapter.children, id);
    if (found) return found;
  }
}

export function reviewTargetIds(chapters: TenderChapter[]): string[] {
  return [...new Set(chapters.flatMap(chapter => [
    ...chapter.targets.filter(target => !target.manual_fill).map(target => target.target_id),
    ...reviewTargetIds(chapter.children),
  ]))];
}

export function editChapter(tree: TenderChapter[], id: string, patch: Partial<TenderChapter>): TenderChapter[] {
  return tree.map(chapter => chapter.chapter_id === id ? { ...chapter, ...patch } : { ...chapter, children: editChapter(chapter.children, id, patch) });
}

export function removeChapter(tree: TenderChapter[], id: string): TenderChapter[] {
  return tree.filter(chapter => chapter.chapter_id !== id).map(chapter => ({ ...chapter, children: removeChapter(chapter.children, id) }));
}

export type DropPosition = "before" | "inside" | "after";
export function moveChapter(tree: TenderChapter[], id: string, destination: string, position: DropPosition): TenderChapter[] {
  const moving = findChapter(tree, id);
  if (!moving || id === destination || findChapter(moving.children, destination) || !findChapter(tree, destination)) return tree;
  const insert = (siblings: TenderChapter[]): TenderChapter[] => siblings.flatMap(chapter => {
    if (chapter.chapter_id !== destination) return [{ ...chapter, children: insert(chapter.children) }];
    if (position === "inside") return [{ ...chapter, children: [...chapter.children, moving] }];
    return position === "before" ? [moving, chapter] : [chapter, moving];
  });
  return insert(removeChapter(tree, id));
}

function location(tree: TenderChapter[], id: string, parents: TenderChapter[] = []): { siblings: TenderChapter[]; index: number; parents: TenderChapter[] } | undefined {
  const index = tree.findIndex(chapter => chapter.chapter_id === id);
  if (index >= 0) return { siblings: tree, index, parents };
  for (const chapter of tree) {
    const found = location(chapter.children, id, [...parents, chapter]);
    if (found) return found;
  }
}

export function moveRelative(tree: TenderChapter[], id: string, direction: "up" | "down" | "indent" | "outdent"): TenderChapter[] {
  const place = location(tree, id);
  if (!place) return tree;
  const { siblings, index, parents } = place;
  if (direction === "outdent") return parents.length ? moveChapter(tree, id, parents[parents.length - 1].chapter_id, "after") : tree;
  const target = siblings[index + (direction === "down" ? 1 : -1)];
  return target ? moveChapter(tree, id, target.chapter_id, direction === "indent" ? "inside" : direction === "down" ? "after" : "before") : tree;
}

export function manualGoal(chapter: TenderChapter): string {
  return chapter.writing_goal ?? chapter.targets.find(target => target.target_id === `manual-response-${chapter.chapter_id}`)?.purpose ?? "";
}

export function canEditManualGoal(chapter: TenderChapter): boolean {
  return chapter.chapter_id.startsWith("manual-") && chapter.targets.every(target => target.target_id === `manual-response-${chapter.chapter_id}`);
}

export function assignTarget(tree: TenderChapter[], target: TenderTarget, destination: string): TenderChapter[] {
  if (!findChapter(tree, destination)) return tree;
  return assignTargetBranch(tree);
  function assignTargetBranch(children: TenderChapter[]): TenderChapter[] {
    return children.map(chapter => {
      const kept = chapter.targets.filter(item => item.target_id !== target.target_id);
      const targets = chapter.chapter_id === destination ? [...kept, target] : kept;
      const changed = kept.length !== chapter.targets.length || chapter.chapter_id === destination;
      return { ...chapter, children: assignTargetBranch(chapter.children), ...(changed ? { targets, target_ids: targets.map(item => item.target_id),
        ...(target.target_id === `manual-response-${chapter.chapter_id}` && chapter.chapter_id !== destination ? { writing_goal: "" } : {}) } : {}) };
    });
  }
}

export function outlineNodes(tree: TenderChapter[]): OutlineNode[] {
  return tree.map(chapter => ({ chapter_id: chapter.chapter_id, title: chapter.title,
    ...(chapter.is_volume !== undefined ? { is_volume: chapter.is_volume } : {}),
    ...(chapter.presentation_role !== undefined ? { presentation_role: chapter.presentation_role } : {}),
    ...(canEditManualGoal(chapter) ? { writing_goal: manualGoal(chapter) } : {}),
    ...(chapter.target_ids ? { target_ids: chapter.targets.map(target => target.target_id) } : {}),
    children: outlineNodes(chapter.children) }));
}
