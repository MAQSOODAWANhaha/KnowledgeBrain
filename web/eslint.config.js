import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";

const rawElements = ["a", "article", "aside", "b", "blockquote", "button", "code", "dd", "details", "div", "dl", "dt", "em", "fieldset",
  "footer", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "i", "img", "input", "label", "legend", "li", "main", "nav",
  "ol", "p", "pre", "section", "select", "small", "span", "strong", "summary", "table", "tbody", "td", "textarea", "th", "thead",
  "tr", "ul"];

// Files still on raw markup. Remove each entry when its migration lands; the list must end empty.
const pendingAstryxMigration = [
  "src/assets/DocumentDetail.tsx",
  "src/assets/FilePreview.tsx",
  "src/bid/authoring/AnalysisProgress.tsx",
  "src/bid/gfm.tsx",
];

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "playwright-report/**",
      "playwright-report-live/**",
      "test-results/**",
      "test-results-live/**",
      "e2e/artifacts/**",
      "postcss.config.cjs",
      "playwright.config.ts",
      "public/onlyoffice-navigation/onlyoffice-plugins-8.3.3.js",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["public/onlyoffice-navigation/*.js"],
    languageOptions: { globals: { window: "readonly", URL: "readonly", MessageEvent: "readonly" } },
  },
  {
    files: ["src/**/*.{ts,tsx}", "e2e/**/*.ts"],
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
    },
  },
  // DESIGN.md §1: page code is built from Astryx components. Raw HTML elements and class-based
  // styling are errors; the ONLYOFFICE frame host is the only exception.
  {
    files: ["src/**/*.tsx"],
    ignores: [...pendingAstryxMigration, "src/bid/authoring/DocxEditor.tsx"],
    rules: {
      "no-restricted-syntax": ["error",
        {
          selector: `JSXOpeningElement[name.name=/^(${rawElements.join("|")})$/]`,
          message: "Use the Astryx component (Text, Heading, Link, Button, List, Card, Section, Collapsible, Table, …) instead of a raw HTML element. See DESIGN.md §6.",
        },
        {
          selector: "JSXAttribute[name.name='className']",
          message: "Style through the theme (web/src/theme.ts) or component props, not CSS classes. See DESIGN.md §1.",
        },
      ],
    },
  },
);
