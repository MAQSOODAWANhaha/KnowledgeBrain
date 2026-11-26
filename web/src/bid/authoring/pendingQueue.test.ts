import type { PendingQueueItem } from "../api/workbench";
import { describe, expect, it } from "./harness";
import { formatDurationMs, groupPendingByLocation, summarizePendingItems } from "./pendingQueue";

function item(partial: Partial<PendingQueueItem> & { gap_id: string }): PendingQueueItem {
  return {
    code: "missing_material",
    detail: "待补说明",
    chapter_id: "ch-1",
    chapter_title: "第一章",
    target_id: "t-1",
    target_purpose: "营业执照",
    bookmark: "bm-1",
    status: "pending",
    note: "",
    created_at: "2026-11-25T10:00:00Z",
    handled_at: null,
    handling_duration_ms: null,
    ...partial,
  };
}

describe("pendingQueue", () => {
  it("groups by response location and filters by status", () => {
    const items = [
      item({ gap_id: "g1", chapter_id: "ch-1", target_id: "t-1", status: "pending" }),
      item({ gap_id: "g2", chapter_id: "ch-1", target_id: "t-1", status: "resolved", handling_duration_ms: 10000 }),
      item({ gap_id: "g3", chapter_id: null, chapter_title: null, target_id: null, target_purpose: null, bookmark: null, status: "need_material", handling_duration_ms: 25000 }),
    ];
    const all = groupPendingByLocation(items, "all");
    expect(all.length).toBe(2);
    expect(all[0].items.map(entry => entry.gap_id).join(",")).toBe("g1,g2");
    expect(all[1].chapterTitle).toBe("全局或未绑定位置");
    expect(groupPendingByLocation(items, "need_material").length).toBe(1);
  });

  it("computes counts and formats duration statistics", () => {
    const items = [
      item({ gap_id: "g1", status: "pending" }),
      item({ gap_id: "g2", status: "resolved", handling_duration_ms: 15000 }),
      item({ gap_id: "g3", status: "accepted_gap", handling_duration_ms: 45000 }),
    ];
    expect(summarizePendingItems(items)).toEqual({
      total: 3,
      pending: 1,
      handled: 2,
      by_status: { pending: 1, resolved: 1, need_material: 0, accepted_gap: 1 },
      handling_duration_ms: { observations: 2, avg: 30000, max: 45000, p95: 45000 },
    });
    expect(formatDurationMs(null)).toBe("—");
    expect(formatDurationMs(400)).toBe("< 1 秒");
    expect(formatDurationMs(45000)).toBe("45 秒");
    expect(formatDurationMs(125000)).toBe("2 分 5 秒");
    expect(formatDurationMs(3660000)).toBe("1 小时 1 分");
  });
});
