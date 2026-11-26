import { describe, expect, it } from "./authoring/harness";
import {
  applyBatchDecision,
  buildHumanIssueEntries,
  filterHumanIssues,
  issueCodeLabel,
  paginate,
  summarizeIssueCodes,
  type Decision,
} from "./analysisReviewHelpers";

describe("analysisReviewHelpers", () => {
  it("builds ordered human issue entries in linear time", () => {
    const entries = buildHumanIssueEntries(
      ["id-2", "id-1", "id-missing"],
      [
        { issue_id: "id-1", code: "semantic_ambiguity", detail: "d1" },
        { issue_id: "id-2", code: "unresolved_cross_reference", detail: "d2" },
      ],
    );
    expect(entries.map(e => [e.id, e.index, e.code, e.issue?.detail])).toEqual([
      ["id-2", 0, "unresolved_cross_reference", "d2"],
      ["id-1", 1, "semantic_ambiguity", "d1"],
      ["id-missing", 2, "unknown", undefined],
    ]);
    expect(issueCodeLabel("unresolved_cross_reference")).toBe("跨处引用未定位（unresolved_cross_reference）");
    expect(issueCodeLabel("custom_code")).toBe("custom_code");
    expect(issueCodeLabel("unknown")).toBe("其他问题");
  });

  it("summarizes issue codes by descending count and tracks decided totals", () => {
    const entries = buildHumanIssueEntries(
      ["a", "b", "c", "d"],
      [
        { issue_id: "a", code: "semantic_ambiguity", detail: "da" },
        { issue_id: "b", code: "unresolved_cross_reference", detail: "db" },
        { issue_id: "c", code: "unresolved_cross_reference", detail: "dc" },
        { issue_id: "d", code: "evidence_quote_mismatch", detail: "dd" },
      ],
    );
    const decisions: Record<string, Decision> = {
      b: { verdict: "dismissed", reason: "已在正文找全" },
      d: { verdict: "acknowledged", reason: "  " },
    };
    expect(summarizeIssueCodes(entries, decisions)).toEqual([
      { code: "unresolved_cross_reference", label: "跨处引用未定位（unresolved_cross_reference）", total: 2, decided: 1 },
      { code: "evidence_quote_mismatch", label: "引文与原文不一致（evidence_quote_mismatch）", total: 1, decided: 0 },
      { code: "semantic_ambiguity", label: "语义歧义（semantic_ambiguity）", total: 1, decided: 0 },
    ]);
  });

  it("filters by issue code and decision status", () => {
    const entries = buildHumanIssueEntries(
      ["a", "b", "c"],
      [
        { issue_id: "a", code: "unresolved_cross_reference", detail: "da" },
        { issue_id: "b", code: "unresolved_cross_reference", detail: "db" },
        { issue_id: "c", code: "semantic_ambiguity", detail: "dc" },
      ],
    );
    const decisions: Record<string, Decision> = {
      a: { verdict: "acknowledged", reason: "手动确认" },
    };
    expect(filterHumanIssues(entries, decisions, "unresolved_cross_reference", "all").map(e => e.id)).toEqual(["a", "b"]);
    expect(filterHumanIssues(entries, decisions, "unresolved_cross_reference", "pending").map(e => e.id)).toEqual(["b"]);
    expect(filterHumanIssues(entries, decisions, "all", "decided").map(e => e.id)).toEqual(["a"]);
  });

  it("applies batch decisions while preserving manual verdicts in undecided mode", () => {
    const entries = buildHumanIssueEntries(
      ["a", "b", "c"],
      [
        { issue_id: "a", code: "unresolved_cross_reference", detail: "da" },
        { issue_id: "b", code: "unresolved_cross_reference", detail: "db" },
        { issue_id: "c", code: "semantic_ambiguity", detail: "dc" },
      ],
    );
    const initial: Record<string, Decision> = {
      a: { verdict: "acknowledged", reason: "人工保留意见" },
      b: { verdict: "dismissed", reason: "  " },
    };
    const crossOnly = filterHumanIssues(entries, initial, "unresolved_cross_reference", "all");
    const undecidedResult = applyBatchDecision(initial, crossOnly, "dismissed", "同一上传文件内可见", "undecided");
    expect(undecidedResult.updated).toBe(1);
    expect(undecidedResult.next.a).toEqual({ verdict: "acknowledged", reason: "人工保留意见" });
    expect(undecidedResult.next.b).toEqual({ verdict: "dismissed", reason: "同一上传文件内可见" });

    const noopResult = applyBatchDecision(undecidedResult.next, crossOnly, "dismissed", "again", "undecided");
    expect(noopResult.updated).toBe(0);
    expect(noopResult.next).toBe(undecidedResult.next);

    const overwriteResult = applyBatchDecision(undecidedResult.next, crossOnly, "dismissed", "统一排除", "all");
    expect(overwriteResult.updated).toBe(2);
    expect(overwriteResult.next.a).toEqual({ verdict: "dismissed", reason: "统一排除" });
  });

  it("paginates items and clamps out-of-range page numbers", () => {
    const items = Array.from({ length: 45 }, (_, idx) => idx + 1);
    expect(paginate(items, 0, 20)).toEqual({ page: 1, totalPages: 3, slice: items.slice(0, 20) });
    expect(paginate(items, 2, 20)).toEqual({ page: 2, totalPages: 3, slice: items.slice(20, 40) });
    expect(paginate(items, 9, 20)).toEqual({ page: 3, totalPages: 3, slice: items.slice(40, 45) });
    expect(paginate([], 3, 20)).toEqual({ page: 1, totalPages: 1, slice: [] });
  });
});
