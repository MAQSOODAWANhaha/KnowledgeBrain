import { Button, Checkbox, FileButton, Select, TextInput } from "@mantine/core";
import type { CompanyProfile, SubmissionProfile } from "../api";

export function MaterialsPane({
  view,
  company,
  submission,
  classifications,
  attachments,
  ended,
  onSaveCompany,
  onSaveSubmission,
  onOverride,
  onResolve,
  onUpload,
  onAttachAction,
}: {
  view: string;
  company: CompanyProfile;
  submission: SubmissionProfile;
  classifications: Array<Record<string, unknown>>;
  attachments: Array<Record<string, unknown>>;
  ended: boolean;
  onSaveCompany: (body: CompanyProfile) => void;
  onSaveSubmission: (body: SubmissionProfile) => void;
  onOverride: (id: string, kind: string, reason: string) => void;
  onResolve: (id: string, resolution: string, attachmentId?: string, reason?: string) => void;
  onUpload: (kind: string, file: File) => void;
  onAttachAction: (id: string, action: string, revision: number) => void;
}) {
  if (view === "company") {
    return (
      <div className="card">
        <h3 className="h3">公司资料</h3>
        {["legal_name", "unified_social_credit_code", "registered_address", "legal_representative", "contact_name", "contact_phone", "contact_email"].map(
          (key) => (
            <TextInput
              key={key}
              mt="sm"
              label={key}
              data-testid={`company-${key}`}
              value={String((company as Record<string, unknown>)[key] ?? "")}
              onChange={(e) => onSaveCompany({ ...company, [key]: e.currentTarget.value })}
            />
          ),
        )}
        <Button mt="md" disabled={ended} onClick={() => onSaveCompany(company)}>
          保存公司资料
        </Button>
      </div>
    );
  }
  if (view === "submission") {
    return (
      <div className="card">
        <h3 className="h3">投标资料</h3>
        {["buyer_name", "project_code", "authorized_representative", "submission_date", "submission_place"].map((key) => (
          <TextInput
            key={key}
            mt="sm"
            label={key}
            data-testid={`submission-${key}`}
            value={String((submission as Record<string, unknown>)[key] ?? "")}
            onChange={(e) => onSaveSubmission({ ...submission, [key]: e.currentTarget.value })}
          />
        ))}
        <Checkbox
          mt="sm"
          label="已确认盖章"
          checked={!!submission.seal_confirmed}
          onChange={(e) => onSaveSubmission({ ...submission, seal_confirmed: e.currentTarget.checked })}
        />
        <Checkbox
          mt="sm"
          label="已确认签字"
          checked={!!submission.signature_confirmed}
          onChange={(e) => onSaveSubmission({ ...submission, signature_confirmed: e.currentTarget.checked })}
        />
        <Button mt="md" disabled={ended} onClick={() => onSaveSubmission(submission)}>
          保存投标资料
        </Button>
      </div>
    );
  }
  return (
    <div className="stack">
      <div className="card">
        <h3 className="h3">程序要求</h3>
        {classifications.map((c) => {
          const id = String(c.id ?? "");
          const requirementKind = String(c.effective_requirement_kind ?? c.router_requirement_kind ?? "");
          const usableAttachments = attachments.filter(
            (attachment) =>
              attachment.kind === requirementKind &&
              attachment.status === "confirmed" &&
              attachment.validation_status === "valid",
          );
          return (
            <div key={id} className="inner" style={{ marginTop: 12 }}>
              <p className="note">
                {String(c.effective_requirement_kind ?? c.router_requirement_kind ?? "review")} · {String(c.router_result_status)}
              </p>
              <div className="row">
                <Select
                  data={["bid_bond", "authorization_support", "seal_sample", "procedural_support", "confirmation"]}
                  placeholder="override kind"
                  onChange={(v) => v && onOverride(id, v, "人工覆盖")}
                />
                <Button size="compact-sm" variant="default" onClick={() => onResolve(id, "confirmed_by_user")}>
                  人工确认
                </Button>
                <Button size="compact-sm" variant="default" onClick={() => onResolve(id, "not_applicable", undefined, "本标不适用")}>
                  不适用
                </Button>
                {usableAttachments.map((attachment) => {
                  const attachmentId = String(attachment.id ?? "");
                  return (
                    <Button
                      key={attachmentId}
                      size="compact-sm"
                      variant="default"
                      data-testid={`resolve-attachment-${id}-${attachmentId}`}
                      onClick={() => onResolve(id, "satisfied_by_attachment", attachmentId)}
                    >
                      用已确认附件满足
                    </Button>
                  );
                })}
              </div>
            </div>
          );
        })}
        {classifications.length === 0 && <p className="note">还没有 current procedural classification。</p>}
      </div>
      <div className="card">
        <h3 className="h3">附件</h3>
        <FileButton onChange={(file) => file && onUpload("authorization_support", file)}>
          {(props) => (
            <Button {...props} variant="default" disabled={ended}>
              上传授权类附件
            </Button>
          )}
        </FileButton>
        {attachments.map((a) => (
          <div key={String(a.id)} data-testid={`attachment-${String(a.id)}`} className="row" style={{ marginTop: 10 }}>
            <span>
              {String(a.kind)} · {String(a.status)} / {String(a.validation_status)}
            </span>
            <Button size="compact-sm" onClick={() => onAttachAction(String(a.id), "validate", Number(a.revision ?? 1))}>
              校验
            </Button>
            <Button size="compact-sm" variant="default" onClick={() => onAttachAction(String(a.id), "confirm", Number(a.revision ?? 1))}>
              确认
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
