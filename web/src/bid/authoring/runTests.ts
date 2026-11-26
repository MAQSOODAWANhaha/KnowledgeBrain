import "../api/http.test";
import "../api/outlineContract.test";
import "./docxSession.test";
import "./compositionReport.test";
import { runAll, testSummary } from "./harness";
import "./sha256.test";
import "./routes.test";
import "./outlineTree.test";
import "./pendingQueue.test";
import "../analysisReviewHelpers.test";

await runAll();
const { failed, passed } = testSummary();
console.log(`${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} tests failed`);
