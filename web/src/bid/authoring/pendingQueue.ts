import type { PendingGapStatus, PendingQueueItem, PendingQueueSummary } from "../api/workbench";

export const PENDING_STATUS_LABELS: Record<PendingGapStatus, string> = {
  pending: "未处理",
  resolved: "已处理",
  need_material: "补资料",
  accepted_gap: "接受空缺",
};

export type PendingLocationGroup = {
  key: string;
  chapterTitle: string;
  targetPurpose: string | null;
  bookmark: string | null;
  items: PendingQueueItem[];
};

export function groupPendingByLocation(
  items: PendingQueueItem[],
  filter: "all" | PendingGapStatus,
): PendingLocationGroup[] {
  const groups: PendingLocationGroup[] = [];
  const byKey = new Map<string, PendingLocationGroup>();
  for (const item of items) {
    if (filter !== "all" && item.status !== filter) continue;
    const key = `${item.chapter_id ?? ""}::${item.target_id ?? ""}`;
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        chapterTitle: item.chapter_title ?? (item.chapter_id ? `章节 ${item.chapter_id}` : "全局或未绑定位置"),
        targetPurpose: item.target_purpose ?? null,
        bookmark: item.bookmark ?? null,
        items: [],
      };
      byKey.set(key, group);
      groups.push(group);
    } else if (!group.bookmark && item.bookmark) {
      group.bookmark = item.bookmark;
    }
    group.items.push(item);
  }
  return groups;
}

export function summarizePendingItems(items: PendingQueueItem[]): PendingQueueSummary {
  const byStatus: Record<PendingGapStatus, number> = {
    pending: 0,
    resolved: 0,
    need_material: 0,
    accepted_gap: 0,
  };
  const durations: number[] = [];
  for (const item of items) {
    byStatus[item.status] += 1;
    if (item.status !== "pending" && typeof item.handling_duration_ms === "number") {
      durations.push(item.handling_duration_ms);
    }
  }
  durations.sort((a, b) => a - b);
  const observations = durations.length;
  const total = items.length;
  const pending = byStatus.pending;
  return {
    total,
    pending,
    handled: total - pending,
    by_status: byStatus,
    handling_duration_ms: {
      observations,
      avg: observations ? Math.round(durations.reduce((acc, value) => acc + value, 0) / observations) : null,
      max: observations ? durations[observations - 1] : null,
      p95: observations ? durations[Math.max(0, Math.ceil(0.95 * observations) - 1)] : null,
    },
  };
}

export function formatDurationMs(ms: number | null | undefined): string {
  if (typeof ms !== "number" || !Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return "< 1 秒";
  const totalSeconds = Math.round(ms / 1000);
  if (totalSeconds < 60) return `${totalSeconds} 秒`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) return seconds ? `${minutes} 分 ${seconds} 秒` : `${minutes} 分`;
  const hours = Math.floor(minutes / 60);
  const remMinutes = minutes % 60;
  return remMinutes ? `${hours} 小时 ${remMinutes} 分` : `${hours} 小时`;
}
