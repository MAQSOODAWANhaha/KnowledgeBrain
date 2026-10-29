import { describe, expect, it } from "./harness";
import { assignTarget, editChapter, findChapter, moveChapter, moveRelative, outlineNodes, removeChapter, reviewTargetIds } from "./outlineTree";
import type { TenderChapter } from "../api/workbench";

const node = (id: string, children: TenderChapter[] = []): TenderChapter => ({ chapter_id: id, title: id, children, targets: [] });

describe("outline arrangement", () => {
  it("rejects self and descendant moves without changing the tree", () => {
    const tree = [node("a", [node("b")]), node("c")];
    expect(moveChapter(tree, "a", "b", "inside")).toBe(tree);
    expect(moveChapter(tree, "a", "a", "after")).toBe(tree);
  });
  it("supports reorder, indent and outdent while preserving identity and references", () => {
    const first = { ...node("a"), targets: [{ target_id: "t", kind: "narrative", purpose: "answer" }] };
    const tree = [first, node("b"), node("c")];
    const indented = moveRelative(tree, "b", "indent");
    expect(indented[0].children[0].chapter_id).toBe("b");
    expect(indented[0].targets).toBe(first.targets);
    expect(moveRelative(indented, "b", "outdent")).toEqual(tree);
    expect(moveRelative(tree, "b", "up")).toEqual(moveChapter(tree, "b", "a", "before"));
    expect(moveRelative(tree, "b", "down")).toEqual(moveChapter(tree, "b", "c", "after"));
  });
  it("edits or removes only the selected branch and accepts an empty outline", () => {
    const tree = [node("a", [node("b")])];
    expect(findChapter(editChapter(tree, "b", { title: "new" }), "b")?.title).toBe("new");
    expect(removeChapter(tree, "a")).toEqual([]);
  });
  it("sends only editable fields and reconstructs only the exact manual writing goal", () => {
    const manual = { ...node("manual-one"), bookmark: "secret", targets: [{ target_id: "manual-response-manual-one", kind: "narrative", purpose: "写实施步骤" }] };
    const ai = { ...node("ai"), writing_goal: "must not replace AI targets", targets: [{ target_id: "target", kind: "narrative", purpose: "original" }] };
    expect(outlineNodes([manual, ai])).toEqual([
      { chapter_id: "manual-one", title: "manual-one", writing_goal: "写实施步骤", children: [] },
      { chapter_id: "ai", title: "ai", children: [] },
    ]);
    expect(outlineNodes([{ ...manual, writing_goal: "" }])[0].writing_goal).toBe("");
  });
  it("moves only target identity while retaining reference data and avoids recreating a moved manual response", () => {
    const target = { target_id: "manual-response-manual-one", kind: "narrative", purpose: "人工目标", rule_refs: [] };
    const tree = [{ ...node("manual-one"), writing_goal: "人工目标", targets: [target] }, node("other")];
    const changed = assignTarget(tree, target, "other");
    expect(changed[0].targets).toEqual([]);
    expect(changed[1].targets[0]).toBe(target);
    expect(outlineNodes(changed)[0].writing_goal).toBe("");
    expect(outlineNodes(changed)[1].target_ids).toEqual([target.target_id]);
    expect(tree[0].targets).toEqual([target]);
    expect(assignTarget(tree, target, "absent")).toBe(tree);
  });
  it("preserves volume and cover attributes through reordering and explicit false/body edits", () => {
    const tree = [{ ...node("volume", [node("content")]), is_volume: true, presentation_role: "cover" }, node("other")];
    const moved = moveChapter(tree, "volume", "other", "after");
    expect(outlineNodes(moved)[1]).toEqual({ chapter_id: "volume", title: "volume", is_volume: true,
      presentation_role: "cover", children: [{ chapter_id: "content", title: "content", children: [] }] });
    expect(outlineNodes(editChapter(moved, "volume", { is_volume: false, presentation_role: "body" }))[1].is_volume).toBe(false);
    expect(tree[0].is_volume).toBe(true);
  });
  it("maps a chapter subtree to real response identities and excludes manual-only content", () => {
    const target = { target_id: "real-response", kind: "narrative", purpose: "说明" };
    const tree = [{ ...node("chapter", [{ ...node("nested"), targets: [target,
      { target_id: "child-response", kind: "form", purpose: "表格" },
      { target_id: "manual-only", kind: "narrative", purpose: "签字", manual_fill: true }] }]), targets: [target] }, node("empty")];
    expect(reviewTargetIds(tree)).toEqual(["real-response", "child-response"]);
    expect(reviewTargetIds([tree[1]])).toEqual([]);
    expect(tree[0].targets[0]).toBe(target);
  });
});
