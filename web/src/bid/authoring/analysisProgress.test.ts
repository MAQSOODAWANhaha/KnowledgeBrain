import { describe, expect, it } from "./harness";
import { analysisSummary } from "./AnalysisProgress";
import type { RequirementSetCompileRequestView } from "../api/types";

function job(
  progress: NonNullable<RequirementSetCompileRequestView["progress"]>,
  status: RequirementSetCompileRequestView["status"] = "pending",
): RequirementSetCompileRequestView {
  return {
    request_artifact_id: "request",
    kind: "RequirementSetCompile",
    progress,
    status,
    request_revision: 1,
    request_sha256: "a",
    frozen_input_sha256: "b",
    document_set_revision_id: "c",
    document_set_sha256: "d",
    disposition_set_revision_id: "e",
    disposition_set_sha256: "f",
    result_identity: null,
    error_code: null,
  };
}

describe("analysis progress copy", () => {
  it("names discover, organize, check, and complete", () => {
    expect(analysisSummary(job({ outline_phase: "discover", turn: 2 }))).toBe("正在发现 · 第 2 步");
    expect(analysisSummary(job({
      outline_phase: "discover",
      outline_pack_failed: 1,
      outline_pack_committed: 1,
      outline_pack_total: 4,
    }))).toBe("正在修正发现");
    expect(analysisSummary(job({ outline_phase: "outline", outline_chapters: 0, turn: 3 }))).toBe(
      "正在组织章节和模板槽 · 已有 0 章 · 第 3 步",
    );
    expect(analysisSummary(job({
      outline_phase: "outline",
      outline_chapters: 2,
      outline_unmapped_forms: 0,
      outline_slots_submitted: true,
    }))).toBe("正在核对 · 已有 2 章");
    expect(analysisSummary(job({ outline_phase: "check", outline_chapters: 2 }))).toBe("正在核对 · 已有 2 章");
    expect(analysisSummary(job({ outline_phase: "complete", outline_chapters: 2 }))).toBe("大纲已完成 · 已有 2 章");
  });

  it("keeps draft-stage fallbacks and drops pack-scan wording", () => {
    expect(analysisSummary(null)).toBe("未开始");
    expect(analysisSummary(job({ draft_stage: "outline", turn: 4 }))).toBe("正在生成章节大纲 · 第 4 步");
    expect(analysisSummary(job({ boundary: "prepared", checkpoint_sequence: 52, draft_stage: "outline" }))).toBe(
      "正在等待模型 · 第 52 步",
    );
    const stale = analysisSummary(job({
      outline_phase: "discover",
      outline_repairing: true,
      outline_pack_committed: 1,
      outline_pack_total: 3,
    }));
    expect(stale.includes("阅读包")).toBe(false);
    expect(stale.includes("语义核对")).toBe(false);
    expect(stale.includes("修补核对")).toBe(false);
    expect(stale.startsWith("正在发现")).toBe(true);
  });
});
