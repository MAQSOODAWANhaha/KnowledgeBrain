export const TENDER_INPUT_EXTENSIONS = [
  ".pdf",
  ".docx",
  ".doc",
  ".xlsx",
  ".xls",
  ".xlsm",
  ".png",
  ".jpg",
  ".jpeg",
  ".webp",
] as const;

export const TENDER_INPUT_ACCEPT = TENDER_INPUT_EXTENSIONS.join(",");
