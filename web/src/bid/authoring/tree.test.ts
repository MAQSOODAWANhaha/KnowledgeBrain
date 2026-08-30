import { describe, expect, it } from "./harness";
import { outlineCandidateQuality } from "./CandidateReview";
import { outlineDisplayTitles } from "./numbering";
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

describe("outlineDisplayTitles", () => {
  it("uses mixed Chinese/Arabic hierarchy and excludes cover/TOC", () => {
    const candidateNodes = [
      ["root", null, 0, "投标文件", "cover", "front_matter"],
      ["toc", "root", 0, "目录", "toc", "toc"],
      ["commercial", "root", 1, "商务文件", "commercial", "section"],
      ["letter", "commercial", 0, "投标函", "commercial", "section"],
      ["authorization", "letter", 0, "授权委托书", "qualification", "section"],
      ["qualification", "commercial", 1, "资格文件", "qualification", "section"],
      ["technical", "root", 2, "技术文件", "technical", "section"],
      ["response", "technical", 0, "技术要求响应", "technical", "section"],
    ].map(([ref, parent, ordinal, title, semanticRole, renderRole]) => ({
      client_node_ref: ref as string,
      parent_client_node_ref: parent as string | null,
      ordinal: ordinal as number,
      title: title as string,
      semantic_role: semanticRole as "cover" | "toc" | "commercial" | "qualification" | "technical",
      render_role: renderRole as "front_matter" | "toc" | "section",
      origin_source_unit_revision_ids: ["11111111-1111-1111-1111-111111111111"],
    }));
    const labels = outlineDisplayTitles(candidateNodes);
    expect(labels.get("root")).toBe("投标文件");
    expect(labels.get("toc")).toBe("目录");
    expect(labels.get("commercial")).toBe("一、商务文件");
    expect(labels.get("letter")).toBe("1. 投标函");
    expect(labels.get("authorization")).toBe("1.1 授权委托书");
    expect(labels.get("qualification")).toBe("2. 资格文件");
    expect(labels.get("technical")).toBe("二、技术文件");
    expect(labels.get("response")).toBe("1. 技术要求响应");

    const quality = outlineCandidateQuality({
      schema_version: 2,
      candidate_id: "candidate",
      kind: "outline",
      status: "proposed",
      base_workspace_revision_id: "revision",
      base_workspace_sha256: "a".repeat(64),
      nodes: candidateNodes,
      bindings: [
        {
          need_occurrence_id: "22222222-2222-2222-2222-222222222222",
          channel: "narrative_content",
          target_client_node_ref: "response",
        },
      ],
      section_obligation_bindings: [
        { obligation_id: "b".repeat(64), target_client_node_ref: "response" },
      ],
      notices: [],
    });
    expect(quality.blocked).toBe(false);
    expect(quality.topLevelCount).toBe(2);
    expect(
      outlineCandidateQuality({
        schema_version: 1,
        candidate_id: "legacy",
        kind: "outline",
        status: "proposed",
        base_workspace_revision_id: "revision",
        base_workspace_sha256: "a".repeat(64),
        nodes: candidateNodes,
        notices: [],
      }).blocked,
    ).toBe(true);
  });
});
