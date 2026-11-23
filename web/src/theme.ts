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
  },
});
