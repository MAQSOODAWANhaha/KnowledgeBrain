import { defineTheme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

// Brand tokens from DESIGN.md §2–3. Components keep their Astryx defaults otherwise.
const family = "Segoe UI";
const fallbacks = '"Microsoft YaHei", "PingFang SC", "Noto Sans SC", -apple-system, BlinkMacSystemFont, sans-serif';

export const brandTheme = defineTheme({
  name: "knowledgebrain",
  extends: neutralTheme,
  typography: {
    scale: { base: 14, ratio: 1.2 },
    body: { family, fallbacks },
    heading: { family, fallbacks },
  },
  radius: { base: 4, multiplier: 1 },
  tokens: {
    "--color-accent": ["#0F6CBD", "#479EF5"],
    "--color-text-accent": ["#0F6CBD", "#479EF5"],
    "--color-icon-accent": ["#0F6CBD", "#479EF5"],
    "--color-on-accent": ["#FFFFFF", "#FFFFFF"],
    "--color-background-body": ["#F3F4F6", "#1B1B1B"],
    "--color-background-surface": ["#FFFFFF", "#242424"],
    "--color-background-card": ["#FFFFFF", "#292929"],
    "--color-text-primary": ["#242424", "#F5F5F5"],
    "--color-text-secondary": ["#616161", "#C7C7C7"],
    "--color-border": ["#E0E0E0", "#3D3D3D"],
    "--color-success": ["#107C10", "#54B054"],
    "--color-warning": ["#BC4B09", "#F98845"],
    "--color-error": ["#C50F1F", "#DC626D"],
  },
  components: {
    // DESIGN.md §2: deep navy app bar; its content renders in the dark color mode (see Shell.tsx).
    "app-shell-header": { base: { backgroundColor: "#0E2A47", borderColor: "#0E2A47" } },
    "top-nav": { base: { backgroundColor: "#0E2A47" } },
    // §2: gray page ground, white page header and panels.
    "layout-content": { base: { backgroundColor: "var(--color-background-body)" } },
    "layout-header": { base: { backgroundColor: "var(--color-background-surface)" } },
    "layout-panel": { base: { backgroundColor: "var(--color-background-surface)" } },
    // Status tags: tinted ground, thin border.
    badge: {
      base: { borderRadius: "4px", borderWidth: "1px", borderStyle: "solid", fontSize: "12px", fontWeight: "600", paddingInline: "8px", minHeight: "22px" },
      "variant:success": { backgroundColor: "#F1FAF1", color: "#107C10", borderColor: "#9FD89F" },
      "variant:warning": { backgroundColor: "#FFF9F5", color: "#BC4B09", borderColor: "#FDCFB4" },
      "variant:error": { backgroundColor: "#FDF3F4", color: "#C50F1F", borderColor: "#EEACB2" },
      "variant:info": { backgroundColor: "#EBF3FC", color: "#0F6CBD", borderColor: "#B4D6FA" },
      "variant:neutral": { backgroundColor: "#F5F5F5", color: "#616161", borderColor: "#E0E0E0" },
    },
    // Message bars: same tints as status tags, thin border, dark text.
    banner: {
      base: { borderWidth: "1px", borderStyle: "solid", borderRadius: "4px", color: "#242424" },
      "status:error": { backgroundColor: "#FDF3F4", borderColor: "#EEACB2" },
      "status:warning": { backgroundColor: "#FFF9F5", borderColor: "#FDCFB4" },
      "status:success": { backgroundColor: "#F1FAF1", borderColor: "#9FD89F" },
      "status:info": { backgroundColor: "#EBF3FC", borderColor: "#B4D6FA" },
    },
    // Fluent buttons: 32 high, semibold; secondary is white with a gray outline.
    button: {
      base: { fontWeight: "600" },
      "variant:secondary": { backgroundColor: "#FFFFFF", borderWidth: "1px", borderStyle: "solid", borderColor: "#D1D1D1", color: "#242424" },
    },
    heading: {
      "level:1": { fontSize: "20px", lineHeight: "28px", fontWeight: "600" },
      "level:2": { fontSize: "16px", lineHeight: "22px", fontWeight: "600" },
    },
    "table-header-cell": { base: { backgroundColor: "#FAFAFA", color: "#616161", fontSize: "13px", fontWeight: "600" } },
    "side-nav-item": {
      base: { fontSize: "13px" },
      selected: { backgroundColor: "#EBF3FC", color: "#0F6CBD", fontWeight: "600" },
    },
    "side-nav": { base: { backgroundColor: "#FBFBFB" } },
    tab: { selected: { color: "#242424", fontWeight: "600" } },
    "tree-list-item": { selected: { backgroundColor: "#EBF3FC", color: "#0F6CBD", fontWeight: "600" } },
  },
});
