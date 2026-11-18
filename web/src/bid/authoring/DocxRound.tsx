import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { FileInput } from "@astryxdesign/core/FileInput";
import { Heading } from "@astryxdesign/core/Heading";
import { Text } from "@astryxdesign/core/Text";
import { useEffect, useState, useSyncExternalStore } from "react";
import { guardHashNavigation } from "../../hash";
import { docxApi } from "../api/docx";
import { createDocxRound } from "./docxRoundSession";

export function DocxRound({ workspaceId, onCancel, onPublished, onUnsafeChange }: {
  workspaceId: string; onCancel: () => void; onPublished: () => void; onUnsafeChange: (value: boolean) => void;
}) {
  const [round] = useState(() => createDocxRound(docxApi, workspaceId));
  const state = useSyncExternalStore(round.subscribe, round.getState);
  useEffect(() => { void round.load(); return round.cancelRead; }, [round]);
  useEffect(() => { onUnsafeChange(round.unsafe()); }, [round, state, onUnsafeChange]);
  useEffect(() => {
    const originalHash = location.hash;
    const unload = (event: BeforeUnloadEvent) => {
      if (round.unsafe()) { event.preventDefault(); event.returnValue = ""; }
    };
    const click = (event: MouseEvent) => {
      if (round.unsafe() && event.target instanceof Element && event.target.closest("a[href]")) {
        event.preventDefault(); event.stopPropagation();
      }
    };
    const unguard = guardHashNavigation(() => {
      if (!round.unsafe()) return true;
      history.replaceState(null, "", `${location.pathname}${location.search}${originalHash}`);
      return false;
    });
    window.addEventListener("beforeunload", unload);
    document.addEventListener("click", click, true);
    return () => { unguard(); window.removeEventListener("beforeunload", unload); document.removeEventListener("click", click, true); };
  }, [round]);
  return <section data-testid="docx-round">
    <Heading level={2}>从 DOCX 创建新轮</Heading>
    <Text>选择已准备好的投标初稿或指定模板。发布后以所选文件开始编制，旧轮保存稿保留，旧轮编辑会话失效。</Text>
    {state.input && <Text>{state.input.expected ? "本次将替换当前编制轮次。" : "本次将创建首份 DOCX 投标稿。"}</Text>}
    <FileInput label="选择 DOCX" accept=".docx" value={state.file} isDisabled={round.unsafe() || state.phase === "published"} onChange={(file) => round.select(Array.isArray(file) ? file[0] ?? null : file)} />
    {state.file && <Text>已选择：{state.file.name}</Text>}
    {state.phase === "loading" && <Text role="status">正在核对新轮依据…</Text>}
    {state.phase === "sending" && <Text role="status">正在发布，请等待结果确认…</Text>}
    {state.error && <Banner status="error" role="alert" title={state.error} />}
    {state.phase === "published" ? <>
      <Text role="status">第 {state.receipt?.round_revision} 轮已发布。</Text>
      <Button variant="primary" onClick={onPublished} label="进入当前稿件" />
    </> : <>
      <Button variant="primary" isDisabled={state.phase !== "uncertain" && (state.phase !== "ready" || !state.file)}
        onClick={() => void round.publish()} label={state.phase === "uncertain" ? "确认发布结果" : "确认创建新轮"} />
      <Button variant="secondary" isDisabled={round.unsafe() || state.phase === "loading"}
        onClick={() => void round.load()} label="刷新依据" />
      <Button variant="secondary" isDisabled={round.unsafe()} onClick={onCancel} label="取消" />
    </>}
  </section>;
}
