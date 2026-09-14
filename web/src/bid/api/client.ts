import { createMutationAttempt, type MutationAttempt } from "../../api";
import { v2Request } from "./http";
import type {
  BidProjectView,
  DocumentRelationKind,
  DocumentRole,
  DocumentSetView,
  ExpectedPointer,
  FreezeDocumentSetResult,
  RequirementSetCompileRequestView,
  RequirementView,
  SourceUnitView,
  TenderDocumentView,
  TenderRelationView,
} from "./types";

export type BidV2Api = {
  listProjects(signal?: AbortSignal): Promise<BidProjectView[]>;
  createProject(
    body: { title: string; ends_at: string },
    attempt?: MutationAttempt,
  ): Promise<BidProjectView>;
  endProject(projectId: string, attempt?: MutationAttempt): Promise<void>;
  getProject(projectId: string, signal?: AbortSignal): Promise<BidProjectView>;
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
  listDocumentSets(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<DocumentSetView[]>;
  freezeDocumentSet(
    projectId: string,
    documentIds: string[],
    expected: ExpectedPointer | null,
    attempt: MutationAttempt,
  ): Promise<FreezeDocumentSetResult>;
  latestRequirementSetCompilation(
    projectId: string,
    signal?: AbortSignal,
  ): Promise<RequirementSetCompileRequestView | null>;
  continueRequirementSetCompilation(
    projectId: string,
    attempt: MutationAttempt,
  ): Promise<RequirementSetCompileRequestView>;
  getRequirementSetCompilation(
    projectId: string,
    requestArtifactId: string,
    signal?: AbortSignal,
  ): Promise<RequirementSetCompileRequestView>;
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
};

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
    async listDocumentSets(projectId, signal) {
      const { data } = await v2Request<
        DocumentSetView[] | { document_sets: DocumentSetView[] }
      >(`/api/v2/bid-projects/${projectId}/document-set-revisions`, { signal });
      return Array.isArray(data) ? data : data.document_sets;
    },
    async freezeDocumentSet(projectId, documentIds, expected, attempt) {
      const { data } = await v2Request<FreezeDocumentSetResult>(
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
    async latestRequirementSetCompilation(projectId, signal) {
      const { data } = await v2Request<RequirementSetCompileRequestView | null>(
        `/api/v2/bid-projects/${projectId}/requirement-set-compilations/latest`, { signal },
      );
      return data;
    },
    async continueRequirementSetCompilation(projectId, attempt) {
      const { data } = await v2Request<RequirementSetCompileRequestView>(
        `/api/v2/bid-projects/${projectId}/requirement-set-compilations/latest/continue`,
        { method: "POST" },
        { attempt },
      );
      return data;
    },
    async getRequirementSetCompilation(projectId, requestArtifactId, signal) {
      const { data } = await v2Request<RequirementSetCompileRequestView>(
        `/api/v2/bid-projects/${projectId}/requirement-set-compilations/${requestArtifactId}`,
        { signal },
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
  };
}

export { createMutationAttempt };
