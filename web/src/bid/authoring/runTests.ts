import "../api/http.test";
import "../api/outlineContract.test";
import "./docxSession.test";
import "./docxRound.test";
import "./compositionReport.test";
import "./fillSession.test";
import "./exportSession.test";
import { runAll, testSummary } from "./harness";
import "./sha256.test";

await runAll();
const { failed, passed } = testSummary();
console.log(`${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} tests failed`);
