import { createMutationAttempt, type MutationAttempt } from "../../api";
import type {
  AcceptCandidateRequest,
  ContentCandidateRequest,
  ExportRequest,
  OutlineCandidateRequest,
} from "../authoring/generation";
import type {
  DocumentSettings,
  WorkspaceMutationRequestV1,
} from "../authoring/mutations";
import { v2Blob, v2Request, type V2RequestOptions } from "./http";
import type {
  AsyncRequestView,
  BidProjectView,
  CandidateView,
  CurrentAssessmentsView,
  DocumentRelationKind,
  DocumentRole,
  EvidenceOverview,
  ExpectedPointer,
  ExportView,
  RequirementView,
  SourceUnitView,
  TenderDocumentView,
  TenderRelationView,
  WorkspaceAssetView,
  WorkspaceEnvelope,
} from "./types";

export type BidV2Api = {
  listProjects(signal?: AbortSignal): Promise<BidProjectView[]>;
  createProject(
    body: { title: string; ends_at: string },
    attempt?: MutationAttempt,
  ): Promise<BidProjectView>;
  endProject(projectId: string, attempt?: MutationAttempt): Promise<void>;
  getProject(projectId: string, signal?: AbortSignal): Promise<BidProjectView>;
  getProjectWorkspace(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<WorkspaceEnvelope>;
  getWorkspace(
    workspaceId: string,
    signal?: AbortSignal,
  ): Promise<WorkspaceEnvelope>;
  mutateWorkspace(
    workspaceId: string,
    body: WorkspaceMutationRequestV1,
    opts: V2RequestOptions,
  ): Promise<WorkspaceEnvelope>;
  listTenderDocuments(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<TenderDocumentView[]>;
  uploadTenderDocument(
    projectId: string,
    file: File,
    attempt: MutationAttempt,
  ): Promise<TenderDocumentView>;
  retryTenderDocument(
    projectId: string,
    documentId: string,
    expectedGeneration: number,
    attempt: MutationAttempt,
  ): Promise<void>;
  patchDocumentRole(
    projectId: string,
    documentId: string,
    role: DocumentRole,
    expected: ExpectedPointer,
    attempt: MutationAttempt,
  ): Promise<TenderDocumentView>;
  freezeDocumentSet(
    projectId: string,
    documentIds: string[],
    expected: ExpectedPointer | null,
    attempt: MutationAttempt,
  ): Promise<ExpectedPointer>;
  listSourceUnits(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<SourceUnitView[]>;
  listRequirements(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<RequirementView[]>;
  listRelations(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<TenderRelationView[]>;
  upsertDocumentRelation(
    projectId: string,
    body: {
      lineage_id?: string;
      from_document_id: string;
      to_document_id: string;
      relation_kind: DocumentRelationKind;
      applicability: Record<string, unknown>;
      expected_artifact_id?: string;
      expected_sha256?: string;
    },
    attempt: MutationAttempt,
  ): Promise<TenderRelationView>;
  createOutlineCandidate(
    workspaceId: string,
    body: OutlineCandidateRequest,
    opts: V2RequestOptions,
  ): Promise<AsyncRequestView>;
  createContentCandidate(
    workspaceId: string,
    body: ContentCandidateRequest,
    opts: V2RequestOptions,
  ): Promise<AsyncRequestView>;
  getRequest(
    workspaceId: string,
    requestArtifactId: string,
    signal?: AbortSignal,
  ): Promise<AsyncRequestView>;
  listWorkspaceRequests(
    workspaceId: string,
    signal?: AbortSignal,
  ): Promise<AsyncRequestView[]>;
  getCandidate(
    workspaceId: string,
    candidateId: string,
    signal?: AbortSignal,
  ): Promise<CandidateView>;
  acceptCandidate(
    workspaceId: string,
    candidateId: string,
    body: AcceptCandidateRequest,
    opts: V2RequestOptions,
  ): Promise<WorkspaceEnvelope>;
  rejectCandidate(
    workspaceId: string,
    candidateId: string,
    opts: V2RequestOptions,
  ): Promise<CandidateView>;
  createOutlineCheckpoint(
    workspaceId: string,
    expected: ExpectedPointer,
    attempt: MutationAttempt,
  ): Promise<ExpectedPointer>;
  matchEvidence(
    workspaceId: string,
    nodeLineageId: string,
    expectedRevisionId: string,
    opts: V2RequestOptions,
  ): Promise<AsyncRequestView>;
  getEvidenceOverview(
    workspaceId: string,
    signal?: AbortSignal,
  ): Promise<EvidenceOverview>;
  getAssessments(
    workspaceId: string,
    signal?: AbortSignal,
  ): Promise<CurrentAssessmentsView>;
  getPreviewHtml(workspaceId: string, signal?: AbortSignal): Promise<string>;
  createExport(
    workspaceId: string,
    body: ExportRequest,
    opts: V2RequestOptions,
  ): Promise<AsyncRequestView>;
  listExports(workspaceId: string, signal?: AbortSignal): Promise<ExportView[]>;
  downloadExport(workspaceId: string, exportId: string): Promise<Blob>;
  listAssets(
    workspaceId: string,
    signal?: AbortSignal,
  ): Promise<WorkspaceAssetView[]>;
  uploadAsset(
    workspaceId: string,
    file: File,
    attempt: MutationAttempt,
  ): Promise<WorkspaceAssetView>;
  patchDocumentSettings(
    workspaceId: string,
    settings: DocumentSettings,
    opts: V2RequestOptions,
  ): Promise<WorkspaceEnvelope>;
};

function envelope(
  workspace: WorkspaceEnvelope["workspace"],
  etag: string | null,
): WorkspaceEnvelope {
  return { workspace, etag: etag || workspace.sha256 };
}

export function createBidV2Client(): BidV2Api {
  return {
    async listProjects(signal) {
      const { data } = await v2Request<
        BidProjectView[] | { projects: BidProjectView[] }
      >("/api/v2/bid-projects", { signal });
      return Array.isArray(data) ? data : data.projects;
    },
    async createProject(body, attempt) {
      const { data } = await v2Request<BidProjectView>(
        "/api/v2/bid-projects",
        { method: "POST", body: JSON.stringify(body) },
        { attempt },
      );
      return data;
    },
    async endProject(projectId, attempt) {
      await v2Request(
        `/api/v2/bid-projects/${projectId}/end`,
        { method: "POST", body: JSON.stringify({}) },
        { attempt },
      );
    },
    async getProject(projectId, signal) {
      const { data } = await v2Request<BidProjectView>(
        `/api/v2/bid-projects/${projectId}`,
        { signal },
      );
      return data;
    },
    async getProjectWorkspace(projectId, signal) {
      const { data, etag } = await v2Request<
        WorkspaceEnvelope["workspace"] | WorkspaceEnvelope
      >(`/api/v2/bid-projects/${projectId}/workspace`, { signal });
      if (data && typeof data === "object" && "workspace" in data) {
        return envelope(data.workspace, data.etag || etag);
      }
      return envelope(data as WorkspaceEnvelope["workspace"], etag);
    },
    async getWorkspace(workspaceId, signal) {
      const { data, etag } = await v2Request<
        WorkspaceEnvelope["workspace"] | WorkspaceEnvelope
      >(`/api/v2/submission-workspaces/${workspaceId}`, { signal });
      if (data && typeof data === "object" && "workspace" in data) {
        return envelope(data.workspace, data.etag || etag);
      }
      return envelope(data as WorkspaceEnvelope["workspace"], etag);
    },
    async mutateWorkspace(workspaceId, body, opts) {
      const { data, etag } = await v2Request<
        WorkspaceEnvelope["workspace"] | WorkspaceEnvelope
      >(
        `/api/v2/submission-workspaces/${workspaceId}/mutations`,
        { method: "POST", body: JSON.stringify(body) },
        { ...opts, ifMatch: opts.ifMatch ?? body.expected_workspace_sha256 },
      );
      if (data && typeof data === "object" && "workspace" in data) {
        return envelope(data.workspace, data.etag || etag);
      }
      return envelope(data as WorkspaceEnvelope["workspace"], etag);
    },
    async listTenderDocuments(projectId, signal) {
      const { data } = await v2Request<
        TenderDocumentView[] | { documents: TenderDocumentView[] }
      >(`/api/v2/bid-projects/${projectId}/tender-documents`, { signal });
      return Array.isArray(data) ? data : data.documents;
    },
    async uploadTenderDocument(projectId, file, attempt) {
      const fd = new FormData();
      fd.set("file", file);
      const { data } = await v2Request<TenderDocumentView>(
        `/api/v2/bid-projects/${projectId}/tender-documents`,
        { method: "POST", body: fd },
        { attempt },
      );
      return data;
    },
    async retryTenderDocument(
      projectId,
      documentId,
      expectedGeneration,
      attempt,
    ) {
      await v2Request(
        `/api/v2/bid-projects/${projectId}/tender-documents/${documentId}/retry`,
        {
          method: "POST",
          body: JSON.stringify({ expected_generation: expectedGeneration }),
        },
        { attempt },
      );
    },
    async patchDocumentRole(projectId, documentId, role, expected, attempt) {
      const { data } = await v2Request<TenderDocumentView>(
        `/api/v2/bid-projects/${projectId}/tender-documents/${documentId}/role`,
        {
          method: "PATCH",
          body: JSON.stringify({
            document_role: role,
            expected_artifact_id: expected.artifact_id,
            expected_sha256: expected.sha256,
          }),
        },
        { attempt },
      );
      return data;
    },
    async freezeDocumentSet(projectId, documentIds, expected, attempt) {
      const { data } = await v2Request<ExpectedPointer>(
        `/api/v2/bid-projects/${projectId}/document-set-revisions`,
        {
          method: "POST",
          body: JSON.stringify({
            document_ids: documentIds,
            expected_artifact_id: expected?.artifact_id ?? null,
            expected_sha256: expected?.sha256 ?? null,
          }),
        },
        { attempt },
      );
      return data;
    },
    async listSourceUnits(projectId, signal) {
      const { data } = await v2Request<
        SourceUnitView[] | { source_units: SourceUnitView[] }
      >(`/api/v2/bid-projects/${projectId}/source-units`, { signal });
      return Array.isArray(data) ? data : data.source_units;
    },
    async listRequirements(projectId, signal) {
      const { data } = await v2Request<
        RequirementView[] | { requirements: RequirementView[] }
      >(`/api/v2/bid-projects/${projectId}/requirements`, { signal });
      return Array.isArray(data) ? data : data.requirements;
    },
    async listRelations(projectId, signal) {
      const { data } = await v2Request<
        TenderRelationView[] | { relations: TenderRelationView[] }
      >(`/api/v2/bid-projects/${projectId}/tender-document-relations`, {
        signal,
      });
      return Array.isArray(data) ? data : data.relations;
    },
    async upsertDocumentRelation(projectId, body, attempt) {
      const { data } = await v2Request<TenderRelationView>(
        `/api/v2/bid-projects/${projectId}/tender-document-relations`,
        { method: "POST", body: JSON.stringify(body) },
        { attempt },
      );
      return data;
    },
    async createOutlineCandidate(workspaceId, body, opts) {
      const { data } = await v2Request<AsyncRequestView>(
        `/api/v2/submission-workspaces/${workspaceId}/outline-candidates`,
        { method: "POST", body: JSON.stringify(body) },
        opts,
      );
      return data;
    },
    async createContentCandidate(workspaceId, body, opts) {
      const { data } = await v2Request<AsyncRequestView>(
        `/api/v2/submission-workspaces/${workspaceId}/content-candidates`,
        { method: "POST", body: JSON.stringify(body) },
        opts,
      );
      return data;
    },
    async getRequest(workspaceId, requestArtifactId, signal) {
      const { data } = await v2Request<AsyncRequestView>(
        `/api/v2/submission-workspaces/${workspaceId}/requests/${requestArtifactId}`,
        { signal },
      );
      return data;
    },
    async listWorkspaceRequests(workspaceId, signal) {
      const { data } = await v2Request<AsyncRequestView[]>(
        `/api/v2/submission-workspaces/${workspaceId}/requests`,
        { signal },
      );
      return Array.isArray(data) ? data : [];
    },
    async getCandidate(workspaceId, candidateId, signal) {
      const { data } = await v2Request<CandidateView>(
        `/api/v2/submission-workspaces/${workspaceId}/candidates/${candidateId}`,
        { signal },
      );
      return data;
    },
    async acceptCandidate(workspaceId, candidateId, body, opts) {
      const { data, etag } = await v2Request<
        WorkspaceEnvelope["workspace"] | WorkspaceEnvelope
      >(
        `/api/v2/submission-workspaces/${workspaceId}/candidates/${candidateId}/accept`,
        { method: "POST", body: JSON.stringify(body) },
        { ...opts, ifMatch: opts.ifMatch ?? body.expected_workspace_sha256 },
      );
      if (data && typeof data === "object" && "workspace" in data) {
        return envelope(data.workspace, data.etag || etag);
      }
      return envelope(data as WorkspaceEnvelope["workspace"], etag);
    },
    async rejectCandidate(workspaceId, candidateId, opts) {
      const { data } = await v2Request<CandidateView>(
        `/api/v2/submission-workspaces/${workspaceId}/candidates/${candidateId}/reject`,
        { method: "POST", body: JSON.stringify({}) },
        opts,
      );
      return data;
    },
    async createOutlineCheckpoint(workspaceId, expected, attempt) {
      const { data } = await v2Request<ExpectedPointer>(
        `/api/v2/submission-workspaces/${workspaceId}/outline-checkpoints`,
        {
          method: "POST",
          body: JSON.stringify({
            expected_workspace_revision_id: expected.artifact_id,
            expected_workspace_sha256: expected.sha256,
          }),
        },
        { attempt },
      );
      return data;
    },
    async matchEvidence(workspaceId, nodeLineageId, expectedRevisionId, opts) {
      const { data } = await v2Request<AsyncRequestView>(
        `/api/v2/submission-workspaces/${workspaceId}/nodes/${nodeLineageId}/evidence-matches`,
        {
          method: "POST",
          body: JSON.stringify({
            expected_workspace_revision_id: expectedRevisionId,
          }),
        },
        opts,
      );
      return data;
    },
    async getEvidenceOverview(workspaceId, signal) {
      const { data } = await v2Request<EvidenceOverview>(
        `/api/v2/submission-workspaces/${workspaceId}/evidence-overview`,
        { signal },
      );
      return data;
    },
    async getAssessments(workspaceId, signal) {
      const { data } = await v2Request<CurrentAssessmentsView>(
        `/api/v2/submission-workspaces/${workspaceId}/assessments/current`,
        { signal },
      );
      return data;
    },
    async getPreviewHtml(workspaceId, signal) {
      const { data } = await v2Request<{ html: string } | string>(
        `/api/v2/submission-workspaces/${workspaceId}/preview?mode=preview`,
        { signal },
      );
      return typeof data === "string" ? data : data.html;
    },
    async createExport(workspaceId, body, opts) {
      const { data } = await v2Request<AsyncRequestView>(
        `/api/v2/submission-workspaces/${workspaceId}/exports`,
        { method: "POST", body: JSON.stringify(body) },
        opts,
      );
      return data;
    },
    async listExports(workspaceId, signal) {
      const { data } = await v2Request<
        ExportView[] | { exports: ExportView[] }
      >(`/api/v2/submission-workspaces/${workspaceId}/exports`, { signal });
      return Array.isArray(data) ? data : data.exports;
    },
    async downloadExport(workspaceId, exportId) {
      return v2Blob(
        `/api/v2/submission-workspaces/${workspaceId}/exports/${exportId}/download`,
      );
    },
    async listAssets(workspaceId, signal) {
      const { data } = await v2Request<
        WorkspaceAssetView[] | { assets: WorkspaceAssetView[] }
      >(`/api/v2/submission-workspaces/${workspaceId}/assets`, { signal });
      return Array.isArray(data) ? data : data.assets;
    },
    async uploadAsset(workspaceId, file, attempt) {
      const fd = new FormData();
      fd.set("file", file);
      const { data } = await v2Request<WorkspaceAssetView>(
        `/api/v2/submission-workspaces/${workspaceId}/assets`,
        { method: "POST", body: fd },
        { attempt },
      );
      return data;
    },
    async patchDocumentSettings(workspaceId, settings, opts) {
      const { data, etag } = await v2Request<
        WorkspaceEnvelope["workspace"] | WorkspaceEnvelope
      >(
        `/api/v2/submission-workspaces/${workspaceId}/document-settings`,
        { method: "PATCH", body: JSON.stringify({ settings }) },
        opts,
      );
      if (data && typeof data === "object" && "workspace" in data) {
        return envelope(data.workspace, data.etag || etag);
      }
      return envelope(data as WorkspaceEnvelope["workspace"], etag);
    },
  };
}

export { createMutationAttempt };
