import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import zhCN from "@astryxdesign/core/locales/zh-CN.generated.js";
import { Theme } from "@astryxdesign/core/theme";
import { brandTheme } from "./theme";
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./app.css";

ReactDOM.createRoot(document.getElementById("app")!).render(
  <React.StrictMode>
    <InternationalizationProvider locale="zh-CN" messages={{ "zh-CN": zhCN }}>
    <Theme theme={brandTheme} mode="light">
    <App />
    </Theme>
    </InternationalizationProvider>
  </React.StrictMode>,
);
