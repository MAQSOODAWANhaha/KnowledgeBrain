import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";

export type KnowledgeSelection = { product_id: string; version_id: string };
type Choice = KnowledgeSelection & { label: string };
const keyOf = (value: KnowledgeSelection) => `${value.product_id}:${value.version_id}`;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function KnowledgeSelection({ value, onChange, disabled = false }: {
  value: KnowledgeSelection[]; onChange(value: KnowledgeSelection[]): void; disabled?: boolean;
}) {
  const [choices, setChoices] = useState<Choice[]>([]);
  const [picked, setPicked] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const load = useCallback(async () => {
    const turn = ++request.current;
    setLoading(true); setError(null);
    try {
      const workspaces = await api.workspaces();
      const result = (await Promise.all(workspaces.map(async workspace => {
        const products = await api.products(workspace.id);
        return (await Promise.all(products.filter(product => uuid.test(product.id)).map(async product => {
          const versions = await api.versions(product.id);
          return versions.filter(version => version.status === "active" && uuid.test(version.id) && version.product_id === product.id)
            .map(version => ({ product_id: product.id, version_id: version.id,
              label: `${workspace.name} / ${product.name} · ${version.label}${product.kind === "library" ? "（资料库）" : ""}` }));
        }))).flat();
      }))).flat();
      if (turn !== request.current) return;
      setChoices(result);
      setPicked(current => result.some(choice => keyOf(choice) === current) ? current : "");
    } catch (ex) {
      if (turn === request.current) setError(ex instanceof Error ? ex.message : "资料目录读取失败，请重试。");
    } finally { if (turn === request.current) setLoading(false); }
  }, []);
  useEffect(() => { const tracker = request; void load(); return () => { tracker.current++; }; }, [load]);
  const chosen = choices.find(choice => keyOf(choice) === picked);
  const duplicate = !!chosen && value.some(item => keyOf(item) === picked);
  const labelOf = (item: KnowledgeSelection) => choices.find(choice => keyOf(choice) === keyOf(item))?.label ?? `资料版本 ${item.version_id}`;
  return <section className="tender-info-card" aria-label="编制参考资料版本">
    <h3>编制参考资料版本</h3>
    <p className="tender-note">选择产品或企业资料的固定版本，供后续编制参考。未选择或资料暂不可读取时，仍可使用项目已上传资料。</p>
    <div className="tender-filter"><select aria-label="选择资料版本" value={picked} disabled={disabled || loading || !!error} style={{ flex: 1, minWidth: 0 }} onChange={event => setPicked(event.target.value)}>
      <option value="">请选择资料版本</option>{choices.map(choice => <option key={keyOf(choice)} value={keyOf(choice)} disabled={value.some(item => keyOf(item) === keyOf(choice))}>{choice.label}</option>)}
    </select><button disabled={disabled || loading || !!error || !chosen || duplicate || value.length >= 20} onClick={() => {
      if (chosen && !duplicate && value.length < 20) { onChange([...value, { product_id: chosen.product_id, version_id: chosen.version_id }]); setPicked(""); }
    }}>添加资料版本</button><button disabled={disabled || loading} onClick={() => void load()}>刷新资料目录</button></div>
    {loading && <p role="status">正在读取可选资料版本…</p>}
    {error && <p role="alert">{error} <button disabled={disabled || loading} onClick={() => void load()}>重试资料目录</button></p>}
    {!loading && !error && !choices.length && <p className="tender-note">当前没有可选的有效资料版本，可先上传项目资料继续编制。</p>}
    {value.length ? <ul aria-label="已选资料版本">{value.map(item => <li key={keyOf(item)}>{labelOf(item)} <button aria-label={`移除 ${labelOf(item)}`} disabled={disabled} onClick={() => onChange(value.filter(existing => keyOf(existing) !== keyOf(item)))}>移除</button></li>)}</ul> : <p className="tender-note">尚未选择参考资料版本。</p>}
    {value.length >= 20 && <p className="tender-note">最多选择 20 个资料版本，请移除部分选择后再添加。</p>}
  </section>;
}
