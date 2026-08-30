import { Node, mergeAttributes } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";

export const BidDocument = Node.create({
  name: "doc",
  topNode: true,
  content: "chapter+",
});

export const ChapterTitle = Node.create({
  name: "chapterTitle",
  content: "inline*",
  defining: true,
  isolating: true,
  addAttributes() {
    return { depth: { default: 0 } };
  },
  parseHTML() {
    return [{ tag: "h1.doc-chapter-title" }, { tag: "h2.doc-chapter-title" }, { tag: "h3.doc-chapter-title" }];
  },
  renderHTML({ node, HTMLAttributes }) {
    const depth = Number(node.attrs.depth ?? 0);
    const tag = depth <= 0 ? "h1" : depth === 1 ? "h2" : "h3";
    return [tag, mergeAttributes({ class: "doc-chapter-title" }, HTMLAttributes), 0];
  },
  addKeyboardShortcuts() {
    return {
      Enter: () =>
        this.editor.commands.command(({ state, dispatch }) => {
          const { $from } = state.selection;
          if ($from.parent.type.name !== "chapterTitle") return false;
          const after = $from.after($from.depth);
          if (dispatch) {
            const paragraph = state.schema.nodes.paragraph.create();
            const tr = state.tr.insert(after, paragraph);
            dispatch(
              tr.setSelection(TextSelection.near(tr.doc.resolve(after + 1))),
            );
          }
          return true;
        }),
    };
  },
});

export const Chapter = Node.create({
  name: "chapter",
  group: "block",
  content: "chapterTitle block+",
  defining: true,
  isolating: true,
  addAttributes() {
    return {
      lineageId: { default: "" },
      depth: { default: 0 },
    };
  },
  parseHTML() {
    return [{ tag: "section[data-chapter]" }];
  },
  renderHTML({ node, HTMLAttributes }) {
    const lineageId = String(node.attrs.lineageId ?? "");
    const depth = Number(node.attrs.depth ?? 0);
    return [
      "section",
      mergeAttributes(HTMLAttributes, {
        class: `doc-chapter depth-${depth}`,
        "data-chapter": lineageId,
        id: lineageId ? `canvas-section-${lineageId}` : undefined,
      }),
      0,
    ];
  },
});

export const PageBreak = Node.create({
  name: "pageBreak",
  group: "block",
  atom: true,
  selectable: true,
  parseHTML() {
    return [{ tag: "div[data-page-break]" }];
  },
  renderHTML() {
    return ["div", { "data-page-break": "", class: "doc-page-break" }, "分页"];
  },
});

export const SignatureBlock = Node.create({
  name: "signature",
  group: "block",
  atom: true,
  selectable: true,
  addAttributes() {
    return {
      signatureKind: { default: "signature" },
      widthMm: { default: 40 },
      heightMm: { default: 20 },
      label: { default: "签字" },
    };
  },
  parseHTML() {
    return [{ tag: "div[data-signature]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      "div",
      mergeAttributes({ "data-signature": "", class: "doc-signature" }, HTMLAttributes),
      String(HTMLAttributes.label ?? "签字"),
    ];
  },
});

export const BidImage = Node.create({
  name: "bidImage",
  group: "block",
  atom: true,
  selectable: true,
  addAttributes() {
    return {
      assetRevisionId: { default: "" },
      widthMm: { default: 80 },
      alignment: { default: "center" },
      alt: { default: "" },
      caption: { default: "" },
      cropLeft: { default: 0 },
      cropTop: { default: 0 },
      cropRight: { default: 0 },
      cropBottom: { default: 0 },
    };
  },
  parseHTML() {
    return [{ tag: "figure[data-bid-image]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      "figure",
      mergeAttributes({ "data-bid-image": "", class: "doc-image" }, HTMLAttributes),
      [
        "figcaption",
        {},
        String(HTMLAttributes.caption || HTMLAttributes.alt || "图片"),
      ],
    ];
  },
});
