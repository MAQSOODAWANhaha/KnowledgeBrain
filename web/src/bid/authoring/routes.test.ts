import { AUTHORING_STEPS, authoringHref, parseAuthoringRoute } from "./routes";
import { expect, it } from "./harness";

it("project routes expose four independent sections and preserve existing URLs", () => {
  expect(AUTHORING_STEPS.map((step) => step.key).join(",")).toBe("files,analysis,authoring,export");
  expect(parseAuthoringRoute("/bids/project")?.step).toBe("files");
  for (const step of AUTHORING_STEPS) {
    const route = parseAuthoringRoute(authoringHref("project", step.key));
    expect(route?.projectId).toBe("project");
    expect(route?.step).toBe(step.key);
  }
  expect(parseAuthoringRoute("/bids/project/authoring/chapter")?.nodeLineageId).toBe("chapter");
  expect(parseAuthoringRoute("/bids/project/analysis/chapter")).toBe(null);
});
