import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { useState } from "react";
import { candidatesFor, contentMatches, slots } from "./model";

export function GuideScreen() {
  const [slotKey, setSlotKey] = useState(slots[0].key);
  const [adopted, setAdopted] = useState<Record<string, string>>({});
  const slot = slots.find((item) => item.key === slotKey) ?? slots[0];
  const candidates = candidatesFor(slot);
  const matched = contentMatches(slot);
  const chosen = adopted[slot.key];

  return (
    <VStack gap={4}>
      <Heading level={2}>按章节引导</Heading>
      <Banner status="info" title="文件名只作标签。向量分数只排序，低分文件仍保留，高分也不会自动采用。是否建议采用只看正文。" />
      <HStack gap={4}>
        <VStack gap={2}>
          <Heading level={3}>本章要填的位置</Heading>
          {slots.map((item) => (
            <Button key={item.key} label={item.title} variant={item.key === slot.key ? "primary" : "secondary"} onClick={() => setSlotKey(item.key)} />
          ))}
        </VStack>
        <VStack gap={2}>
          <Heading level={3}>正文建议</Heading>
          <Text>{slot.purpose}</Text>
          {matched.length === 0 && <Text>没有正文对得上这个位置。</Text>}
          {matched.length > 1 && <Text>有多份正文对得上，不会自动采用。</Text>}
          {matched.map((document) => (
            <Card key={document.documentId}>
              <VStack gap={2}>
                <Text>{document.title}</Text>
                <Text>{document.text}</Text>
                <Button label={chosen === document.documentId ? "已采用" : "采用这份正文"} variant="primary" onClick={() => setAdopted((current) => ({ ...current, [slot.key]: document.documentId }))} />
              </VStack>
            </Card>
          ))}
          {chosen && <Text>当前采用的是文件编号，不是文件名。</Text>}
        </VStack>
        <VStack gap={2}>
          <Heading level={3}>{slot.library === "product" ? "产品库" : "企业库"}全部文件</Heading>
          {candidates.map((document) => (
            <Card key={document.documentId}>
              <HStack gap={2}>
                <Text>{document.title}</Text>
                <Badge label={document.score.toFixed(2)} />
              </HStack>
            </Card>
          ))}
        </VStack>
      </HStack>
    </VStack>
  );
}
