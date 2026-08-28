import { useEffect, useRef, useState } from "react";
import { createBidV2Client } from "../api/client";
import type { AuthoringRoute } from "./routes";
import {
  createBidV2Session,
  shouldPoll,
  type BidV2Session,
  type BidV2State,
} from "./session";

function browserClock() {
  return {
    now: () => Date.now(),
    schedule: (fn: () => void, ms: number) => {
      const timer = window.setTimeout(fn, ms);
      return () => window.clearTimeout(timer);
    },
  };
}

function createBrowserSession(): BidV2Session {
  return createBidV2Session({
    api: createBidV2Client(),
    clock: browserClock(),
  });
}

export function useBidV2Session(route: AuthoringRoute | null): {
  session: BidV2Session;
  state: BidV2State;
} {
  const sessionRef = useRef<BidV2Session | null>(null);
  if (sessionRef.current === null) sessionRef.current = createBrowserSession();
  const session = sessionRef.current;
  const [state, setState] = useState(() => session.getState());
  const poll = shouldPoll(state);
  const projectId = route?.projectId ?? null;
  const step = route?.step ?? null;
  const nodeLineageId = route?.nodeLineageId ?? null;

  useEffect(
    () => session.subscribe(() => setState(session.getState())),
    [session],
  );

  useEffect(() => {
    if (!projectId || !step) return;
    void session.applyRoute({ projectId, step, nodeLineageId });
  }, [session, projectId, step, nodeLineageId]);

  useEffect(() => {
    if (!poll) return;
    const timer = window.setInterval(() => void session.refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [session, poll]);

  useEffect(
    () => () => {
      session.dispose();
      sessionRef.current = null;
    },
    [session],
  );

  return { session, state };
}
