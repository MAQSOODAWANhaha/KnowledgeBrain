import { randomUuid } from "../../uuid";

export type WordNavigationRequest = { id: string; versionId: string; bookmark: string };
export type WordNavigationResult = { id: string; ok: boolean; reason?: string };
const channel = "kb.word-navigation.v1";

// The navigation plugin runs inside the editor and on the application's origin.
// Both ancestry and a per-editor nonce are checked before accepting its replies.
export function connectWordNavigation({ elementId, editorKey, versionId, onResult }: {
  elementId: string; editorKey: string; versionId: string;
  onResult(result: WordNavigationResult): void;
}) {
  const nonce = randomUuid();
  let plugin: Window | null = null;
  let disposed = false;
  let pending: { request: WordNavigationRequest; timer: ReturnType<typeof setTimeout> } | null = null;
  const origin = window.location.origin;
  const send = (data: object) => plugin?.postMessage({ channel, nonce, editorKey, versionId, ...data }, origin);
  const belongsToEditor = (source: MessageEventSource | null): source is Window => {
    const element = document.getElementById(elementId);
    const frame = element instanceof HTMLIFrameElement ? element : element?.querySelector("iframe");
    if (!frame?.contentWindow || !source || !("parent" in source)) return false;
    let parent = source as Window;
    try {
      for (let depth = 0; depth < 8; depth++) {
        if (parent === frame.contentWindow) return true;
        if (parent === parent.parent) return false;
        parent = parent.parent;
      }
    } catch { return false; }
    return false;
  };
  const finish = (ok: boolean, reason?: string) => {
    if (!pending) return;
    const id = pending.request.id;
    clearTimeout(pending.timer); pending = null;
    onResult({ id, ok, ...(reason ? { reason } : {}) });
  };
  const receive = (event: MessageEvent) => {
    if (disposed || event.origin !== origin || !belongsToEditor(event.source)) return;
    const data = event.data;
    if (!data || data.channel !== channel) return;
    if (data.type === "hello") {
      plugin = event.source;
      const element = document.getElementById(elementId);
      const frame = element instanceof HTMLIFrameElement ? element : element?.querySelector("iframe");
      if (frame?.src) send({ type: "bind", editorOrigin: new URL(frame.src).origin });
      return;
    }
    if (event.source !== plugin || data.nonce !== nonce || data.editorKey !== editorKey || data.versionId !== versionId) return;
    if (data.type === "ready" && pending) send({ type: "navigate", ...pending.request });
    if (data.type === "result" && data.id === pending?.request.id) {
      finish(data.ok === true, data.ok === true ? undefined : "Word 中的书签已删除或暂不可定位，请在 Word 内查找并核对。");
    }
  };
  window.addEventListener("message", receive);
  return {
    navigate(request: WordNavigationRequest) {
      if (disposed) return;
      if (request.versionId !== versionId || !/^kb_[ct]_[a-f0-9]{28}$/.test(request.bookmark)) {
        onResult({ id: request.id, ok: false, reason: "目录与当前 Word 版本不一致，未跳转。" });
        return;
      }
      if (pending) finish(false, "已选择另一处位置。");
      pending = { request, timer: setTimeout(() => finish(false, "Word 定位插件尚未响应，可先使用 Word 自带导航核对。"), 5000) };
      send({ type: "navigate", ...request });
    },
    dispose() {
      send({ type: "unbind" });
      disposed = true;
      window.removeEventListener("message", receive);
      if (pending) clearTimeout(pending.timer);
      pending = null; plugin = null;
    },
  };
}
