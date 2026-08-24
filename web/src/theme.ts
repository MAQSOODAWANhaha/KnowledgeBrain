import { createTheme } from "@mantine/core";

const lake = [
  "#F3F8FE",
  "#E8F1FC",
  "#D0E3F8",
  "#A8C8F0",
  "#7AABE6",
  "#4E8FDC",
  "#1D6FD8",
  "#185BBB",
  "#144A99",
  "#0F3A78",
] as const;

export const theme = createTheme({
  fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Noto Sans SC", sans-serif',
  fontFamilyMonospace: 'ui-monospace, "SF Mono", "Menlo", monospace',
  headings: {
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Noto Sans SC", sans-serif',
    fontWeight: "600",
  },
  defaultRadius: 10,
  primaryColor: "lake",
  primaryShade: 6,
  fontSizes: { xs: "13px", sm: "13px", md: "15px", lg: "17px", xl: "22px" },
  colors: {
    lake,
  },
  components: {
    Button: {
      defaultProps: { size: "sm", radius: 10 },
      styles: { root: { fontWeight: 600, height: 36 } },
    },
    TextInput: { defaultProps: { size: "md", radius: 10, variant: "filled" } },
    PasswordInput: { defaultProps: { size: "md", radius: 10, variant: "filled" } },
    Textarea: { defaultProps: { size: "md", radius: 10, variant: "filled" } },
    Select: { defaultProps: { size: "md", radius: 10, variant: "filled" } },
    Modal: { defaultProps: { radius: 16, centered: true, size: 440 } },
    Badge: { defaultProps: { size: "sm", variant: "light", radius: "xl" } },
    Notification: { defaultProps: { radius: 12 } },
  },
});
