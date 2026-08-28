export type AssessmentStatus =
  | "ready"
  | "has_warnings"
  | "has_critical_warnings";

export type AssessmentIssue = {
  issue_id: string;
  code: string;
  severity: "info" | "warning" | "high";
  message: string;
};

export type AssessmentSnapshot = {
  assessment_snapshot_id: string;
  status: AssessmentStatus;
  issues: AssessmentIssue[];
};

export type CurrentAssessments = {
  outline: AssessmentSnapshot | null;
  submission: AssessmentSnapshot | null;
};

export function checkpointAllowed(_assessment: AssessmentSnapshot | null): {
  allowed: true;
} {
  return { allowed: true };
}

export function exportAllowed(args: {
  assessment: AssessmentSnapshot | null;
  technicalReady: boolean;
}): { allowed: boolean; reason?: string } {
  if (!args.technicalReady) return { allowed: false, reason: "technical" };
  return { allowed: true };
}

export function assessmentBlocksUi(
  _status: AssessmentStatus | null | undefined,
): false {
  return false;
}
