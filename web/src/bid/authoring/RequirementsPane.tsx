import { Button, Select } from "@mantine/core";
import { useState } from "react";
import {
  DOCUMENT_RELATIONS,
  DOCUMENT_ROLES,
  type DocumentRelationKind,
  type DocumentRole,
} from "../api/types";
import { DOCUMENT_ROLE_LABEL } from "../helpers";
import type { BidV2Session, BidV2State } from "./session";

export function RequirementsPane({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const expected = state.workspace?.document_set_revision_id
    ? {
        artifact_id: state.workspace.document_set_revision_id,
        sha256: state.workspace.document_set_sha256 ?? "",
      }
    : null;
  const [fromDocumentId, setFromDocumentId] = useState<string | null>(null);
  const [toDocumentId, setToDocumentId] = useState<string | null>(null);
  const [relationKind, setRelationKind] =
    useState<DocumentRelationKind>("complements");
  const documentsReady =
    state.documents.length > 0 &&
    state.documents.every(
      (doc) => doc.parse_status === "ready" || doc.parse_status === "completed",
    );
  const documentOptions = state.documents.map((doc) => ({
    value: doc.id,
    label: doc.file_name,
  }));
  return (
    <div className="stack">
      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h3 className="h3">文件角色与冻结</h3>
          <Button
            data-testid="freeze-document-set"
            disabled={state.ended || !documentsReady}
            onClick={() =>
              void session.freezeDocumentSet(
                state.documents.map((doc) => doc.id),
                expected,
              )
            }
          >
            冻结文件集
          </Button>
        </div>
        <p className="note">
          确认文件角色和关系；所有文件解析完成后冻结文件集并编译要求台账。
        </p>
        <table className="grid">
          <thead>
            <tr>
              <th>文件</th>
              <th>角色</th>
              <th>解析</th>
            </tr>
          </thead>
          <tbody>
            {state.documents.map((doc) => (
              <tr key={doc.id}>
                <td>{doc.file_name}</td>
                <td>
                  <Select
                    size="xs"
                    data={DOCUMENT_ROLES.map((role) => ({
                      value: role,
                      label: DOCUMENT_ROLE_LABEL[role] ?? role,
                    }))}
                    value={doc.document_role}
                    allowDeselect={false}
                    disabled={state.ended}
                    onChange={(value) => {
                      if (!value) return;
                      void session.setDocumentRole(
                        doc.id,
                        value as DocumentRole,
                        {
                          artifact_id: doc.role_revision_id,
                          sha256: doc.role_revision_sha256,
                        },
                      );
                    }}
                  />
                </td>
                <td>{doc.parse_status}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <h4 className="h3">文件关系</h4>
        <div className="row">
          <Select
            placeholder="来源文件"
            data={documentOptions}
            value={fromDocumentId}
            onChange={setFromDocumentId}
            disabled={state.ended}
          />
          <Select
            placeholder="目标文件"
            data={documentOptions.filter(
              (option) => option.value !== fromDocumentId,
            )}
            value={toDocumentId}
            onChange={setToDocumentId}
            disabled={state.ended}
          />
          <Select
            data={DOCUMENT_RELATIONS.map((kind) => ({
              value: kind,
              label: kind,
            }))}
            value={relationKind}
            allowDeselect={false}
            onChange={(value) =>
              value && setRelationKind(value as DocumentRelationKind)
            }
            disabled={state.ended}
          />
          <Button
            disabled={
              state.ended ||
              !fromDocumentId ||
              !toDocumentId ||
              fromDocumentId === toDocumentId
            }
            onClick={() => {
              if (!fromDocumentId || !toDocumentId) return;
              void session.addDocumentRelation(
                fromDocumentId,
                toDocumentId,
                relationKind,
              );
            }}
          >
            确认关系
          </Button>
        </div>
        {state.relations.map((relation) => (
          <div key={relation.lineage_id} className="note">
            {state.documents.find(
              (doc) => doc.id === relation.from_document_id,
            )?.file_name ?? relation.from_document_id}
            {" → "}
            {relation.relation_kind}
            {" → "}
            {state.documents.find((doc) => doc.id === relation.to_document_id)
              ?.file_name ?? relation.to_document_id}
          </div>
        ))}
      </div>
      <div className="card">
        <h3 className="h3">来源单元</h3>
        {state.sourceUnits.length === 0 && (
          <p className="note">
            冻结并完成解析后，这里列出章节/表格/表单/OCR 区域。
          </p>
        )}
        {state.sourceUnits.slice(0, 80).map((unit) => (
          <div key={unit.source_unit_revision_id} className="note">
            {unit.kind} · {unit.disposition} · {unit.text.slice(0, 80)}
          </div>
        ))}
      </div>
      <div className="card">
        <h3 className="h3">要求台账</h3>
        {state.requirements.length === 0 && (
          <p className="note">还没有 ProjectRequirementSet。</p>
        )}
        {state.requirements.map((req) => (
          <div key={req.requirement_revision_id} className="item">
            <div>
              <div className="name">{req.text}</div>
              <div className="desc">
                {req.requiredness} · {req.compliance_policy} · {req.lifecycle}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
