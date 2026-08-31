import { describe, expect, it } from "./harness";
import { OWNER_MVP_EVIDENCE_MODE_OPTIONS } from "./generation";

describe("owner-only MVP evidence mode", () => {
  it("does not expose an unusable user PickSet selector", () => {
    expect(OWNER_MVP_EVIDENCE_MODE_OPTIONS).toEqual([
      { value: "system_proposed", label: "系统建议" },
    ]);
  });
});
