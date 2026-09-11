import "../api/http.test";
import "./docxSession.test";
import "./docxRound.test";
import "./compositionSession.test";
import { runAll, testSummary } from "./harness";
import "./sha256.test";

await runAll();
const { failed, passed } = testSummary();
console.log(`${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} tests failed`);
