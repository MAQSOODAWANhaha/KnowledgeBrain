import { createTheme } from "@mantine/core";

const iris = [
  "#f7f6ff",
  "#ecebfe",
  "#d4d2fb",
  "#b3b0f7",
  "#8f8bf4",
  "#746ff2",
  "#635ff2",
  "#514cdc",
  "#433fba",
  "#353292",
] as const;

export const theme = createTheme({
  fontFamily: '"Plus Jakarta Sans", "Noto Sans SC", "PingFang SC", sans-serif',
  fontFamilyMonospace: 'ui-monospace, "SF Mono", "JetBrains Mono", monospace',
  headings: {
    fontFamily: '"Plus Jakarta Sans", "Noto Sans SC", "PingFang SC", sans-serif',
    fontWeight: "700",
  },
  defaultRadius: 12,
  primaryColor: "iris",
  primaryShade: 6,
  fontSizes: { xs: "12.5px", sm: "13.5px", md: "14px", lg: "16px", xl: "22px" },
  colors: {
    iris,
    lake: iris,
  },
  components: {
    Button: {
      defaultProps: { size: "sm", radius: 12 },
      styles: { root: { fontWeight: 600, height: 36 } },
    },
    TextInput: { defaultProps: { size: "md", radius: 12, variant: "filled" } },
    PasswordInput: { defaultProps: { size: "md", radius: 12, variant: "filled" } },
    Textarea: { defaultProps: { size: "md", radius: 12, variant: "filled" } },
    Modal: { defaultProps: { radius: 16, centered: true } },
    Badge: { defaultProps: { size: "sm", variant: "light", radius: "xl" } },
    Notification: { defaultProps: { radius: 12 } },
  },
});
