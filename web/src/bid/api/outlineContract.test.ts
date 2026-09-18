import { describe, expect, it } from "../authoring/harness";
import {
  isBodyStatus,
  isChapterPurpose,
  isOutlineBlockerCode,
  isOutlineIssue,
  isOutlineRequirement,
  type OutlineDraftPlanItem,
} from "./types";

const fixtureChapter: OutlineDraftPlanItem = {
  id: "chapter-1",
  parent: null,
  order: 0,
  title: "投标函",
  prescribed: true,
  purpose: "response",
  requirement_ids: ["requirement-1"],
  format_refs: [],
  body_status: "empty",
  grounds: [{ source_id: "source", start: 0, end: 12 }],
  status: "pending",
};

const fixtureRequirement = {
  description: "投标函",
  kind: "submission",
  applicability: "required",
  condition: "",
  grounds: [{ source_id: "source", start: 0, end: 12 }],
  format_grounds: [],
  order_constraints: [],
};

const fixtureIssue = {
  code: "hint",
  requirement_ids: ["requirement-1"],
  chapter_ids: ["chapter-1"],
  reference_ids: [],
  grounds: [],
  status: "open",
  resolution_grounds: [],
};

describe("outline contract DTO guards", () => {
  it("accepts fixture-shaped outline chapter and requirement objects", () => {
    expect(isChapterPurpose(fixtureChapter.purpose)).toBe(true);
    expect(isBodyStatus(fixtureChapter.body_status)).toBe(true);
    expect(isOutlineRequirement(fixtureRequirement)).toBe(true);
    expect(isOutlineIssue(fixtureIssue)).toBe(true);
    expect(isOutlineBlockerCode("B_SCAN_INCOMPLETE")).toBe(true);
    expect(Array.isArray(fixtureChapter.requirement_ids)).toBe(true);
    expect(Array.isArray(fixtureChapter.format_refs)).toBe(true);
  });

  it("rejects legacy outline shapes", () => {
    expect(isChapterPurpose("leaf")).toBe(false);
    expect(isBodyStatus("pending")).toBe(false);
    expect(isOutlineBlockerCode("Omitted")).toBe(false);
    expect(isOutlineRequirement({ description: "x" })).toBe(false);
    expect(isOutlineIssue("old string issue")).toBe(false);
  });
});
