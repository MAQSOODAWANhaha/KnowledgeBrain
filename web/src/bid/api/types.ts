export type FrozenIdentity = {
  artifact_id: string;
  sha256: string;
};

export type BidProjectView = {
  id: string;
  title: string;
  status: string;
  ended_at: string | null;
  ends_at?: string | null;
  workspace_id: string;
};

export const DOCUMENT_ROLES = [
  "primary_tender",
  "bid_format",
  "technical_specification",
  "commercial_requirement",
  "bill_of_quantities",
  "contract",
  "drawing",
  "clarification",
  "amendment",
  "other_attachment",
] as const;
export type DocumentRole = (typeof DOCUMENT_ROLES)[number];

export const DOCUMENT_RELATIONS = [
  "complements",
  "clarifies",
  "partially_amends",
  "replaces",
  "withdraws",
] as const;
export type DocumentRelationKind = (typeof DOCUMENT_RELATIONS)[number];

export type TenderDocumentView = {
  id: string;
  project_id: string;
  file_name: string;
  media_type: string;
  byte_length: number;
  document_role: DocumentRole;
  role_revision_id: string;
  role_revision_sha256: string;
  role_provenance: "system_suggested" | "human_confirmed" | "human_modified";
  parse_status: "pending" | "processing" | "ready" | "completed" | "failed";
  conversion_generation: number;
  error_code: string | null;
  original_sha256: string;
};

export type DocumentSetView = FrozenIdentity & {
  revision: number;
  items: Array<{
    document_id: string;
    ordinal: number;
    role_revision_id: string;
    source_revision_id: string | null;
    disposition: "ready" | "pending" | "failed" | "unresolved";
  }>;
};

export type FreezeDocumentSetResult = FrozenIdentity & {
  revision: number;
  disposition_set_artifact_id: string;
  disposition_set_sha256: string;
  request_artifact_id: string;
  request_revision: number;
  request_sha256: string;
  frozen_input_sha256: string;
};

export type RequirementCompileResultIdentity = {
  status: "succeeded";
  published_current: boolean;
  workspace_apply_required: boolean;
  requirement_set_id: string;
  requirement_set_sha256: string;
  document_set_revision_id: string;
  document_set_sha256: string;
  requirement_count: number;
  requirement_projection_id?: string;
  requirement_projection_sha256?: string;
  compiler_version: 3;
  replayed: boolean;
};

export type RequirementSetCompileRequestView = {
  request_artifact_id: string;
  kind: "RequirementSetCompile";
  progress?: {
    phase?: string;
    turn?: number;
    records?: number;
    tool_calls?: number;
    review_rounds?: number;
    checkpoint_sequence?: number;
    boundary?: string;
    draft_stage?: string;
    outline_phase?: string;
    outline_chapters?: number;
    outline_scan_cursor?: number;
    outline_scan_chunks?: number;
    outline_scan_repair?: boolean;
    outline_requirements?: number;
    outline_open_issues?: number;
  } | null;
  status: "pending" | "succeeded" | "failed";
  request_revision: number;
  request_sha256: string;
  frozen_input_sha256: string;
  document_set_revision_id: string;
  document_set_sha256: string;
  disposition_set_revision_id: string;
  disposition_set_sha256: string;
  result_identity: RequirementCompileResultIdentity | null;
  error_code: string | null;
};

export type TenderRelationView = {
  lineage_id: string;
  revision_id: string;
  revision_sha256: string;
  from_document_id: string;
  to_document_id: string;
  relation_kind: DocumentRelationKind;
  applicability: Record<string, unknown>;
};

export type SourceUnitDisposition =
  "requirement" | "non_requirement" | "unresolved";

export type OutlineNode = {
  title: string;
  kind: string;
  children?: OutlineNode[];
  record_id?: string;
};

export type TenderOutline = {
  notices?: string[];
  quality: "draft";
  compile_status?: string | null;
  extracted_from: "none" | "checkpoint" | "published";
  extracted?: OutlineNode[];
  documents?: Array<{
    id: string;
    file_name: string;
    parse_status: string;
    source?: OutlineNode[];
  }>;
};

export type SourceUnitView = {
  source_unit_revision_id: string;
  document_id: string;
  kind:
    | "section"
    | "table_row"
    | "form_region"
    | "attachment_region"
    | "image_ocr_region";
  disposition: SourceUnitDisposition;
  text: string;
};

export type RequirementView = {
  requirement_revision_id: string;
  lineage_id: string;
  text: string;
  requiredness: "mandatory" | "optional" | "informational";
  compliance_policy:
    "must_comply" | "explicit_response" | "deviation_allowed" | "scored";
  lifecycle: "current" | "superseded" | "withdrawn";
  source_unit_revision_ids: string[];
};

export type ExpectedPointer = FrozenIdentity;

export type ChapterPurpose = "group" | "response";
export type BodyStatus = "empty" | "user" | "generated";

export type OutlineBlockerCode =
  | "B_SCAN_INCOMPLETE"
  | "B_REQUIREMENT_UNMAPPED"
  | "B_TREE_INVALID"
  | "B_CONFLICT_FALSELY_RESOLVED"
  | "B_FORMAT_EVIDENCE_MISSING";

export type OutlineRequirement = {
  description: string;
  kind: string;
  applicability: "required" | "conditional" | "not_applicable";
  condition: string;
  grounds: Array<Record<string, unknown>>;
  format_grounds: Array<Record<string, unknown>>;
  order_constraints: string[];
};

export type OutlineIssue = {
  code: string;
  requirement_ids: string[];
  chapter_ids: string[];
  reference_ids: string[];
  grounds: Array<Record<string, unknown>>;
  status: "open" | "resolved";
  resolution_grounds: Array<Record<string, unknown>>;
};

export type OutlineDraftPlanItem = {
  id: string;
  parent: string | null;
  order: number;
  title: string;
  prescribed: boolean;
  purpose: ChapterPurpose;
  requirement_ids: string[];
  format_refs: Array<Record<string, unknown>>;
  body_status: BodyStatus;
  grounds: Array<Record<string, unknown>>;
  status: "pending" | "filled" | "omitted";
};

export const OUTLINE_BLOCKER_CODES: readonly OutlineBlockerCode[] = [
  "B_SCAN_INCOMPLETE",
  "B_REQUIREMENT_UNMAPPED",
  "B_TREE_INVALID",
  "B_CONFLICT_FALSELY_RESOLVED",
  "B_FORMAT_EVIDENCE_MISSING",
] as const;

export function isChapterPurpose(value: unknown): value is ChapterPurpose {
  return value === "group" || value === "response";
}

export function isBodyStatus(value: unknown): value is BodyStatus {
  return value === "empty" || value === "user" || value === "generated";
}

export function isOutlineBlockerCode(value: unknown): value is OutlineBlockerCode {
  return typeof value === "string"
    && (OUTLINE_BLOCKER_CODES as readonly string[]).includes(value);
}

export function isOutlineRequirement(value: unknown): value is OutlineRequirement {
  if (!value || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  return typeof row.description === "string"
    && typeof row.kind === "string"
    && (row.applicability === "required"
      || row.applicability === "conditional"
      || row.applicability === "not_applicable")
    && typeof row.condition === "string"
    && Array.isArray(row.grounds)
    && Array.isArray(row.format_grounds)
    && Array.isArray(row.order_constraints);
}

export function isOutlineIssue(value: unknown): value is OutlineIssue {
  if (!value || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  return typeof row.code === "string"
    && Array.isArray(row.requirement_ids)
    && Array.isArray(row.chapter_ids)
    && Array.isArray(row.reference_ids)
    && Array.isArray(row.grounds)
    && (row.status === "open" || row.status === "resolved")
    && Array.isArray(row.resolution_grounds);
}
