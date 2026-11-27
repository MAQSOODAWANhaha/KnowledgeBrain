import { useCallback, useEffect, useRef, useState } from "react";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack } from "@astryxdesign/core/HStack";
import { Selector } from "@astryxdesign/core/Selector";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { Toolbar } from "@astryxdesign/core/Toolbar";
import { VStack } from "@astryxdesign/core/VStack";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../api";
import { tenderApi, type TenderSourceDocument, type TenderSourceRole, type TenderSourceSettings, type TenderSourceSettingsSave } from "./api/workbench";

const roleOptions: Array<{ value: TenderSourceRole; label: string }> = [
  { value: "main", label: "主件" },
  { value: "attachment", label: "附件" },
  { value: "clarification", label: "澄清" },
  { value: "amendment", label: "补遗" },
];
type Draft = { version: string; documents: TenderSourceDocument[]; missing: string };
type Pending = { body: TenderSourceSettingsSave; attempt: MutationAttempt };
const fromSettings = (settings: TenderSourceSettings): Draft => ({
  version: settings.version, documents: settings.documents, missing: settings.expected_document_ids.join("\n"),
});

export function SourceSettings({ projectId, active, readOnly, disabled, filesVersion, selectedFileId, onChanged, onUnsafeChange }: {
  projectId: string; active: boolean; readOnly: boolean; disabled: boolean; filesVersion: string; selectedFileId?: string;
  onChanged(): void; onUnsafeChange(value: boolean): void;
}) {
  const [remote, setRemote] = useState<TenderSourceSettings | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [saving, setSaving] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [saved, setSaved] = useState(false);
  const request = useRef(0);
  const writing = useRef(false);
  const read = useCallback(async (signal?: AbortSignal, discard = false) => {
    if (writing.current) return;
    const turn = ++request.current;
    try {
      const latest = await tenderApi.sourceSettings(projectId, signal);
      if (signal?.aborted || turn !== request.current) return;
      setRemote(latest);
      setReadError(null);
      if (discard) {
        setDraft(null); setPending(null); setConflict(false); setSaveError(null); setSaved(false);
      }
    } catch (error) {
      if (!signal?.aborted && turn === request.current) {
        setReadError(error instanceof Error ? error.message : "读取来源设置失败。");
      }
    }
  }, [projectId]);
  useEffect(() => {
    if (!active) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      await read(controller.signal);
      if (!controller.signal.aborted) timer = setTimeout(() => void poll(), 5000);
    };
    void poll();
    return () => { controller.abort(); clearTimeout(timer); };
  }, [active, filesVersion, read]);

  const unsafe = !!draft || !!pending || saving;
  useEffect(() => { onUnsafeChange(unsafe); }, [unsafe, onUnsafeChange]);
  const current = draft ?? (remote && fromSettings(remote));
  const changed = !!draft && !!remote && draft.version !== remote.version;
  const locked = readOnly || disabled || saving || !!pending;
  const selected = current?.documents.find(document => document.document_id === selectedFileId);
  function edit(update: (working: Draft) => Draft) {
    if (locked || !remote) return;
    setDraft(previous => update(previous ?? fromSettings(remote)));
    setSaved(false);
  }
  function editFile(patch: Partial<Pick<TenderSourceDocument, "role" | "replaces" | "modification_scope">>) {
    edit(working => ({ ...working, documents: working.documents.map(document => document.document_id === selectedFileId
      ? { ...document, ...patch } : document) }));
  }
  async function save() {
    if (readOnly || disabled || writing.current || !current || (!pending && (changed || conflict || !!readError))) return;
    const flight = pending ?? {
      attempt: createMutationAttempt(),
      body: {
        expected_version: current.version,
        documents: current.documents.map(({ document_id, role, replaces, modification_scope }) => ({
          document_id, role, replaces, modification_scope: modification_scope?.trim() || null,
        })),
        expected_document_ids: current.missing.split(/\r?\n/).map(value => value.trim()).filter(Boolean),
      },
    };
    writing.current = true;
    request.current++;
    setPending(flight); setSaving(true); setSaveError(null); setSaved(false);
    try {
      const latest = await tenderApi.saveSourceSettings(projectId, flight.body, flight.attempt);
      setRemote(latest); setDraft(null); setPending(null); setConflict(false); setSaved(true);
      onChanged();
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        setPending(null);
        setConflict(error.status === 409);
        setSaveError(error.message);
      } else setSaveError("来源设置的保存结果尚未确认，请确认同一次请求。");
    } finally {
      writing.current = false;
      setSaving(false);
    }
  }

  return <Card padding={0} role="region" aria-label="来源设置">
    <VStack gap={0}>
      <Toolbar label="来源设置标题" dividers={["bottom"]} startContent={<Text weight="semibold">来源设置</Text>} />
      <VStack gap={3} padding={4}>
        {readError && <Banner role="alert" status="error" title={readError} endContent={<Button size="sm" label="重试读取来源设置" onClick={() => void read()} />} />}
        {(saveError || changed) && <Banner role="alert" status={conflict || changed ? "warning" : "error"}
          title={saveError ?? "来源设置已变化，请核对最新设置。"} />}
        {saved && <Text role="status">来源设置已保存。</Text>}
        {!current && !readError && <Spinner label="正在读取来源设置…" />}
        {current && <>
          {!readOnly && <Text type="supporting">逐份核对文件角色并保存。新文件上传后也需核对。必要附件补齐后，请移除对应缺件登记。</Text>}
          {selected && <>
            <Selector label="文件角色" value={selected.role} isDisabled={locked} options={roleOptions}
              onChange={role => editFile({ role: role as TenderSourceRole })} />
            <Selector label="替代文件" value={selected.replaces ?? ""} isDisabled={locked}
              options={[{ value: "", label: "无明确替代文件" }, ...current.documents.filter(document => document.document_id !== selected.document_id)
                .map(document => ({ value: document.document_id, label: document.file_name }))]}
              onChange={replaces => editFile({ replaces: replaces || null })} />
            <TextArea label="适用范围" rows={2} htmlMaxLength={2000} isDisabled={locked} value={selected.modification_scope ?? ""}
              onChange={modification_scope => editFile({ modification_scope })} />
          </>}
          <TextArea label="必要缺件（每行一项）" rows={3} isDisabled={locked} value={current.missing}
            onChange={missing => edit(working => ({ ...working, missing }))} />
          <HStack gap={2} wrap="wrap">
            {!readOnly && <Button variant="primary" label={pending ? "确认同一次来源设置保存" : "保存来源设置"}
              isDisabled={disabled || saving || !pending && (changed || conflict || !!readError)}
              onClick={() => void save()} />}
            {(draft || pending || conflict) && <Button variant="secondary" label="放弃修改并载入最新设置" isDisabled={saving}
              onClick={() => { if (window.confirm("放弃当前修改并载入服务器上的来源设置？")) void read(undefined, true); }} />}
          </HStack>
        </>}
      </VStack>
    </VStack>
  </Card>;
}
