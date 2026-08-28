import { emptyRichText } from "./contentBlock";
import {
  clearAckedDrafts,
  hasDrafts,
  upsertDraft,
  type DraftMap,
} from "./drafts";
import { describe, expect, it } from "./harness";

function draft(id: string, generation?: number) {
  return {
    generation,
    nodeLineageId: "n",
    blockLineageId: id,
    op: "update" as const,
    ordinal: 0,
    block: {
      schema_version: 1 as const,
      block_revision_id: id,
      lineage_id: id,
      revision: 1,
      origin: "human" as const,
      dependency_sha256: null,
      content_sha256: "0".repeat(64),
      kind: "rich_text" as const,
      content: emptyRichText(),
    },
    baseWorkspaceRevisionId: "w",
  };
}

describe("drafts", () => {
  it("keeps newer generations when clearing acks", () => {
    let drafts: DraftMap = {};
    drafts = upsertDraft(drafts, draft("a"));
    drafts = upsertDraft(drafts, draft("a"));
    expect(drafts.a?.generation).toBe(2);
    const remaining = clearAckedDrafts(drafts, { a: 1 });
    expect(remaining.a?.generation).toBe(2);
    expect(hasDrafts(clearAckedDrafts(drafts, { a: 2 }))).toBe(false);
  });
});
