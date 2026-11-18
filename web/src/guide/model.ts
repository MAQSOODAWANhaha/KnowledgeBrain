export type GuideDocument = {
  documentId: string;
  title: string;
  kind: "library" | "product";
  text: string;
  score: number;
};

export type GuideSlot = {
  key: string;
  title: string;
  purpose: string;
  library: "library" | "product";
};

export const slots: GuideSlot[] = [
  { key: "license", title: "营业执照", purpose: "营业执照", library: "library" },
  { key: "certs", title: "资质证书", purpose: "资质证书", library: "library" },
  { key: "performance", title: "类似业绩", purpose: "近三年类似业绩", library: "library" },
  { key: "service", title: "售后服务", purpose: "售后服务", library: "library" },
  { key: "product", title: "技术方案", purpose: "产品功能与组网", library: "product" },
];

export const documents: GuideDocument[] = [
  { documentId: "license-real", title: "营业执照.pdf", kind: "library", text: "营业执照。统一社会信用代码。", score: 0.88 },
  { documentId: "license-mock", title: "模拟验收营业执照.png", kind: "library", text: "营业执照。模拟验收。", score: 0.88 },
  { documentId: "license-trap", title: "营业执照-名称陷阱.md", kind: "library", text: "会议纪要。下周讨论日程，没有证照信息。", score: 0 },
  { documentId: "record", title: "记录002.md", kind: "library", text: "统一社会信用代码 TEST-NOT-REAL-0000。类型：营业执照。", score: 0.57 },
  { documentId: "profile", title: "企业基本信息.md", kind: "library", text: "企业基本情况介绍。", score: 0.2 },
  { documentId: "case", title: "业绩合同-测试电网边界安全改造项目.pdf", kind: "library", text: "甲方与乙方签订合同。合同金额以测试数据记载。", score: 0.05 },
  { documentId: "service-named", title: "售后服务制度-测试.md", kind: "library", text: "售后响应时间：下一工作日。", score: 0.76 },
  { documentId: "scan", title: "扫描件001.md", kind: "library", text: "售后响应时间：4小时到场。", score: 0.47 },
  { documentId: "paper", title: "产品技术白皮书.md", kind: "product", text: "产品功能、组网和部署说明。", score: 0.7 },
  { documentId: "product-cert", title: "商用密码产品认证证书-NGFW-X3000.pdf", kind: "product", text: "认证证书。产品型号 NGFW-X3000。", score: 0.4 },
];

function markers(purpose: string): string[] {
  const found: string[] = [];
  if (purpose.includes("执照")) found.push("统一社会信用代码", "营业执照");
  if (purpose.includes("业绩")) found.push("甲方", "合同金额");
  if (purpose.includes("服务")) found.push("响应时间", "售后");
  if ((purpose.includes("证书") || purpose.includes("认证") || purpose.includes("资质")) && !purpose.includes("执照")) found.push("认证证书");
  return found;
}

export function candidatesFor(slot: GuideSlot): GuideDocument[] {
  return documents
    .filter((document) => document.kind === slot.library)
    .slice()
    .sort((left, right) => right.score - left.score || left.title.localeCompare(right.title, "zh"));
}

export function contentMatches(slot: GuideSlot): GuideDocument[] {
  const terms = markers(slot.purpose);
  if (!terms.length) return [];
  return candidatesFor(slot).filter((document) => terms.some((term) => document.text.includes(term)));
}
