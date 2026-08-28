import { describe, expect, it } from "./harness";
import {
  dropMove,
  dropPlacementFromRatio,
  flattenPreorder,
  outlineIndex,
  type OutlineNodeView,
} from "./tree";

function node(
  id: string,
  parent: string | null,
  ordinal: number,
): OutlineNodeView {
  return {
    lineage_id: id,
    revision_id: id,
    parent_lineage_id: parent,
    ordinal,
    title: id,
    semantic_role: "other",
    render_role: "section",
    stale: false,
    block_lineage_ids: [],
  };
}

const tree = outlineIndex([
  node("root", null, 0),
  node("a", "root", 0),
  node("b", "root", 1),
  node("c", "root", 2),
  node("a1", "a", 0),
]);

describe("flattenPreorder", () => {
  it("walks depth-first", () => {
    expect(
      flattenPreorder(tree).map((item) => `${item.depth}:${item.lineage_id}`),
    ).toEqual(["0:root", "1:a", "2:a1", "1:b", "1:c"]);
  });
});

describe("dropMove", () => {
  it("places before a sibling using post-remove ordinals", () => {
    expect(dropMove(tree, "c", "a", "before")).toEqual({
      parentLineageId: "root",
      ordinal: 0,
    });
  });

  it("places after a sibling", () => {
    expect(dropMove(tree, "a", "b", "after")).toEqual({
      parentLineageId: "root",
      ordinal: 1,
    });
  });

  it("nests as last child", () => {
    expect(dropMove(tree, "b", "a", "child")).toEqual({
      parentLineageId: "a",
      ordinal: 1,
    });
  });

  it("rejects moving a node into its descendant", () => {
    expect(() => dropMove(tree, "a", "a1", "child")).toThrow(/子孙/);
  });
});

describe("dropPlacementFromRatio", () => {
  it("splits a row into before / child / after", () => {
    expect(dropPlacementFromRatio(0.1)).toBe("before");
    expect(dropPlacementFromRatio(0.5)).toBe("child");
    expect(dropPlacementFromRatio(0.9)).toBe("after");
  });
});
