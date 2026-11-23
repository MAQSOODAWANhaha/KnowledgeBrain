import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import zhCN from "@astryxdesign/core/locales/zh-CN.generated.js";
import { Theme } from "@astryxdesign/core/theme";
import { brandTheme } from "./theme";
import React from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { App } from "./App";
import "./app.css";

ReactDOM.createRoot(document.getElementById("app")!).render(
  <React.StrictMode>
    <InternationalizationProvider locale="zh-CN" messages={{ "zh-CN": zhCN }}>
    <Theme theme={brandTheme} mode="light">
    <App />
    <Toaster
      position="top-center"
      duration={4000}
      toastOptions={{
        className:
          "rounded-[8px] border border-line bg-white text-ink shadow-elev",
      }}
    />
    </Theme>
    </InternationalizationProvider>
  </React.StrictMode>,
);
