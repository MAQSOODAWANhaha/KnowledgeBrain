import "./drafts.test";
import { runAll, testSummary } from "./harness";
import "./session.test";
import "./tree.test";

await runAll();
const { failed, passed } = testSummary();
console.log(`${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} tests failed`);
