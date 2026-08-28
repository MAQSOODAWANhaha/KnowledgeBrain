import { CandidateReview } from "./CandidateReview";
import { DocumentCanvas } from "./DocumentCanvas";
import type { BidV2Session, BidV2State } from "./session";

export function AuthoringShell({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  return (
    <>
      <CandidateReview session={session} state={state} />
      <DocumentCanvas session={session} state={state} />
    </>
  );
}
