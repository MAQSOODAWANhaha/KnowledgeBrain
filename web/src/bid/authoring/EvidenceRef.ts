import { Mark, mergeAttributes } from "@tiptap/core";

export const EvidenceRef = Mark.create({
  name: "evidenceRef",
  addAttributes() {
    return {
      evidenceBundleId: { default: null },
      evidenceItemId: { default: null },
      quoteStartOffset: { default: 0 },
      quoteEndOffset: { default: 1 },
    };
  },
  parseHTML() {
    return [{ tag: "span[data-evidence-ref]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      "span",
      mergeAttributes({ "data-evidence-ref": "" }, HTMLAttributes),
      0,
    ];
  },
});
