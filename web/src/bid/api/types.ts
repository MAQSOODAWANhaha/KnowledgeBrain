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
