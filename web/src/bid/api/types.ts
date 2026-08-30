import type { CurrentAssessments } from "../authoring/assessment";
import type { ContentBlockV1 } from "../authoring/contentBlock";
import type { DocumentSettings } from "../authoring/mutations";
import type { OutlineNodeView } from "../authoring/tree";

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
  parse_status:
    | "pending"
    | "processing"
    | "ready"
    | "completed"
    | "failed";
  conversion_generation: number;
  error_code: string | null;
  original_sha256: string;
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
  | "requirement"
  | "non_requirement"
  | "unresolved";

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
    | "must_comply"
    | "explicit_response"
    | "deviation_allowed"
    | "scored";
  lifecycle: "current" | "superseded" | "withdrawn";
  source_unit_revision_ids: string[];
};

export type FulfillmentBindingView = {
  binding_lineage_id: string;
  need_occurrence_id: string;
  node_lineage_id: string | null;
  stale: boolean;
};

export type WorkspaceView = {
  workspace_id: string;
  project_id: string;
  revision_id: string;
  sha256: string;
  scope: "project_wide";
  outline_checkpoint_id: string | null;
  outline_checkpoint_sha256: string | null;
  requirement_projection_revision_id: string;
  requirement_projection_sha256: string;
  document_settings_revision_id: string;
  document_settings_sha256: string;
  document_settings: DocumentSettings;
  document_set_revision_id: string | null;
  document_set_sha256: string | null;
  nodes: OutlineNodeView[];
  blocks: ContentBlockV1[];
  bindings: FulfillmentBindingView[];
  quote_snapshot: FrozenIdentity | null;
};

export type WorkspaceEnvelope = {
  workspace: WorkspaceView;
  etag: string;
};

export type AsyncRequestKind =
  | "TenderDocumentProcess"
  | "RequirementSetCompile"
  | "OutlineGenerate"
  | "ContentGenerate"
  | "SubmissionExport";

export type OutlineProgressDetail = {
  phase?:
    | "analyzing"
    | "mapping"
    | "reducing"
    | "collecting"
    | "drafting"
    | "routing"
    | "verifying"
    | "repairing"
    | "publishing"
    | "retrying"
    | "succeeded"
    | "failed"
    | "cancelled";
  attempt?: number;
  max_attempts?: number;
  retry_count?: number;
  mapped_batches?: number;
  total_batches?: number;
  structure_signals?: number;
  requirements_done?: number;
  requirements_total?: number;
  turn_in_attempt?: number;
  tool_calls_in_attempt?: number;
  total_turns?: number;
  total_tool_calls?: number;
  text_bytes_read?: number;
  images_read?: number;
  last_error_code?: string | null;
  [key: string]: unknown;
};

export type OutlineProgress = {
  stage?: string;
  label?: string;
  status?: "running" | "succeeded" | "failed" | "cancelled";
  sequence?: number;
  detail?: OutlineProgressDetail;
};

export type AsyncRequestView = {
  request_artifact_id: string;
  kind: AsyncRequestKind;
  status: "pending" | "succeeded" | "failed" | "obsolete";
  result_identity?: FrozenIdentity | null;
  error_code?: string | null;
  progress?: OutlineProgress | null;
};

export type ContentCandidateOperation = {
  kind: "insert_block" | "append_to_block" | "insert_at_anchor";
  client_operation_ref: string;
  block: ContentBlockV1;
};

export type ContentCandidateView = {
  candidate_id: string;
  kind: "content";
  status: "proposed" | "accepted" | "rejected" | "obsolete";
  base_workspace_revision_id: string;
  base_workspace_sha256: string;
  operations: ContentCandidateOperation[];
  notices: Array<{ code: string; message: string }>;
};

export type OutlineCandidateView = {
  schema_version?: 1 | 2;
  candidate_id: string;
  kind: "outline";
  status: "proposed" | "accepted" | "rejected" | "obsolete";
  base_workspace_revision_id: string;
  base_workspace_sha256: string;
  nodes: Array<{
    client_node_ref: string;
    parent_client_node_ref: string | null;
    ordinal: number;
    title: string;
    semantic_role?:
      | "cover"
      | "toc"
      | "qualification"
      | "technical"
      | "commercial"
      | "quotation"
      | "deviation"
      | "implementation"
      | "evidence_index"
      | "attachment"
      | "other";
    render_role?: "section" | "front_matter" | "toc" | "appendix" | "hidden";
    origin_source_unit_revision_ids?: string[];
  }>;
  bindings?: Array<{
    need_occurrence_id: string;
    channel: string;
    target_client_node_ref: string;
  }>;
  section_obligation_bindings?: Array<{
    obligation_id: string;
    target_client_node_ref: string;
  }>;
  notices: Array<{ code: string; message: string; severity: string }>;
};

export type CandidateView = ContentCandidateView | OutlineCandidateView;

export type ExportView = {
  export_id: string;
  mode: "review_draft" | "submission";
  format: "docx" | "pdf";
  status: "pending" | "succeeded" | "failed" | "obsolete";
};

export type WorkspaceAssetView = {
  asset_revision_id: string;
  media_type: string;
  file_name: string;
  byte_length: number;
};

export type EvidenceOverview = {
  node_lineage_id: string | null;
  covered_requirement_ids: string[];
  missing_requirement_ids: string[];
  bundles: Array<{ evidence_bundle_id: string; title: string }>;
};

export type ExpectedPointer = FrozenIdentity;

export type CurrentAssessmentsView = CurrentAssessments;
