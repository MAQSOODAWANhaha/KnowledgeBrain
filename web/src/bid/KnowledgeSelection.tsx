import { List, ListItem } from "@astryxdesign/core/List";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
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
  return <VStack role="region" aria-label="编制参考资料版本" gap={3}>
    <VStack gap={1}><Heading level={2}>编制参考资料版本</Heading>
      <Text type="supporting">选择产品或企业资料的固定版本，供后续编制参考。未选择或资料暂不可读取时，仍可使用项目已上传资料。</Text></VStack>
    <HStack gap={2} vAlign="end" wrap="wrap">
      <Selector label="选择资料版本" value={picked} isDisabled={disabled || loading || !!error} onChange={setPicked} options={[{ value: "", label: "请选择资料版本" }, ...choices.map(choice => ({ value: keyOf(choice), label: choice.label, disabled: value.some(item => keyOf(item) === keyOf(choice)) }))]} />
      <Button variant="primary" isDisabled={disabled || loading || !!error || !chosen || duplicate || value.length >= 20} onClick={() => {
        if (chosen && !duplicate && value.length < 20) { onChange([...value, { product_id: chosen.product_id, version_id: chosen.version_id }]); setPicked(""); }
      }} label="添加资料版本" />
      <Button variant="secondary" isDisabled={disabled || loading} onClick={() => void load()} label="刷新资料目录" />
    </HStack>
    {loading && <Text role="status" type="supporting">正在读取可选资料版本…</Text>}
    {error && <Banner role="alert" status="error" title="资料目录读取失败，可先用项目已上传资料继续编制。" description={error} endContent={<Button size="sm" isDisabled={disabled || loading} onClick={() => void load()} label="重试资料目录" />} />}
    {!loading && !error && !choices.length && <Text type="supporting">当前没有可选的有效资料版本，可先上传项目资料继续编制。</Text>}
    {value.length ? <List aria-label="已选资料版本" density="compact" hasDividers>{value.map(item => <ListItem key={keyOf(item)} startContent={<Badge variant="info" label="已选" />} label={labelOf(item)}
      endContent={<Button size="sm" variant="ghost" aria-label={`移除 ${labelOf(item)}`} isDisabled={disabled} onClick={() => onChange(value.filter(existing => keyOf(existing) !== keyOf(item)))} label="移除" />} />)}</List> : <Text type="supporting">尚未选择参考资料版本。</Text>}
    {value.length >= 20 && <Text type="supporting">最多选择 20 个资料版本，请移除部分选择后再添加。</Text>}
  </VStack>;
}
