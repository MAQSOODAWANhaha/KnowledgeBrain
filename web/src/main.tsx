import { Theme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral/built";
import React from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { App } from "./App";
import "./app.css";

ReactDOM.createRoot(document.getElementById("app")!).render(
  <React.StrictMode>
    <Theme theme={neutralTheme}>
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
  </React.StrictMode>,
);
