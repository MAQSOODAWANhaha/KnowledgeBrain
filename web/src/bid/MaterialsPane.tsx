import { useState } from "react";
import { Button, Checkbox, FileButton, Select, TextInput } from "@mantine/core";
import type {
  AttachmentAction,
  AttachmentKind,
  CompanyProfile,
  ProceduralAttachment,
  ProceduralClassification,
  ProceduralRequirementKind,
  ProceduralResolution,
  SubmissionProfile,
} from "../api";

const ATTACHMENT_KINDS: Array<{ value: AttachmentKind; label: string }> = [
  { value: "bid_bond", label: "投标保证金" },
  { value: "authorization_support", label: "授权证明" },
  { value: "seal_sample", label: "盖章样本" },
  { value: "procedural_support", label: "其他程序材料" },
];

const COMPANY_FIELDS: Array<{ key: Exclude<keyof CompanyProfile, "revision">; label: string }> = [
  { key: "legal_name", label: "公司名称" },
  { key: "unified_social_credit_code", label: "统一社会信用代码" },
  { key: "registered_address", label: "注册地址" },
  { key: "legal_representative", label: "法定代表人" },
  { key: "contact_name", label: "联系人" },
  { key: "contact_phone", label: "联系电话" },
  { key: "contact_email", label: "联系邮箱" },
];

type SubmissionTextField = "buyer_name" | "project_code" | "authorized_representative" | "submission_date" | "submission_place";

const SUBMISSION_FIELDS: Array<{ key: SubmissionTextField; label: string }> = [
  { key: "buyer_name", label: "采购人" },
  { key: "project_code", label: "项目编号" },
  { key: "authorized_representative", label: "授权代表" },
  { key: "submission_date", label: "递交日期" },
  { key: "submission_place", label: "递交地点" },
];

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
  classifications: ProceduralClassification[];
  attachments: ProceduralAttachment[];
  ended: boolean;
  onSaveCompany: (body: CompanyProfile) => void;
  onSaveSubmission: (body: SubmissionProfile) => void;
  onOverride: (id: string, kind: ProceduralRequirementKind, reason: string) => void;
  onResolve: (id: string, resolution: ProceduralResolution, attachmentId?: string, reason?: string) => void;
  onUpload: (kind: AttachmentKind, file: File) => void;
  onAttachAction: (id: string, action: AttachmentAction, revision: number) => void;
}) {
  const [attachmentKind, setAttachmentKind] = useState<AttachmentKind>("authorization_support");

  if (view === "company") {
    return (
      <div className="card">
        <h3 className="h3">公司资料</h3>
        {COMPANY_FIELDS.map((field) => (
          <TextInput
            key={field.key}
            mt="sm"
            label={field.label}
            data-testid={`company-${field.key}`}
            value={company[field.key] ?? ""}
            onChange={(e) => onSaveCompany({ ...company, [field.key]: e.currentTarget.value })}
          />
        ))}
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
        {SUBMISSION_FIELDS.map((field) => (
          <TextInput
            key={field.key}
            mt="sm"
            label={field.label}
            data-testid={`submission-${field.key}`}
            value={submission[field.key] ?? ""}
            onChange={(e) => onSaveSubmission({ ...submission, [field.key]: e.currentTarget.value })}
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
          const requirementKind = c.effective_requirement_kind ?? c.router_requirement_kind;
          const usableAttachments = attachments.filter(
            (attachment) =>
              attachment.kind === requirementKind &&
              attachment.status === "confirmed" &&
              attachment.validation_status === "valid",
          );
          return (
            <div key={c.id} className="inner" style={{ marginTop: 12 }}>
              <p>{c.segment_text}</p>
              <p className="note">
                {requirementKind} · {c.router_result_status}
              </p>
              <div className="row">
                <Select
                  data={["bid_bond", "authorization_support", "seal_sample", "procedural_support", "confirmation"]}
                  placeholder="override kind"
                  onChange={(value) => value && onOverride(c.id, value as ProceduralRequirementKind, "人工覆盖")}
                />
                <Button size="compact-sm" variant="default" onClick={() => onResolve(c.id, "confirmed_by_user")}>
                  人工确认
                </Button>
                <Button size="compact-sm" variant="default" onClick={() => onResolve(c.id, "not_applicable", undefined, "本标不适用")}>
                  不适用
                </Button>
                {usableAttachments.map((attachment) => {
                  return (
                    <Button
                      key={attachment.id}
                      size="compact-sm"
                      variant="default"
                      data-testid={`resolve-attachment-${c.id}-${attachment.id}`}
                      onClick={() => onResolve(c.id, "satisfied_by_attachment", attachment.id)}
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
        <div className="row">
          <Select
            data-testid="attachment-kind"
            label="材料分类"
            data={ATTACHMENT_KINDS}
            value={attachmentKind}
            allowDeselect={false}
            onChange={(value) => value && setAttachmentKind(value as AttachmentKind)}
          />
          <FileButton onChange={(file) => file && onUpload(attachmentKind, file)}>
            {(props) => (
              <Button {...props} data-testid="attachment-upload" variant="default" disabled={ended}>
                上传附件
              </Button>
            )}
          </FileButton>
        </div>
        {attachments.map((a) => (
          <div key={a.id} data-testid={`attachment-${a.id}`} className="row" style={{ marginTop: 10 }}>
            <span>
              {a.kind} · {a.status} / {a.validation_status}
            </span>
            <Button size="compact-sm" onClick={() => onAttachAction(a.id, "validate", a.revision)}>
              校验
            </Button>
            <Button size="compact-sm" variant="default" onClick={() => onAttachAction(a.id, "confirm", a.revision)}>
              确认
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
