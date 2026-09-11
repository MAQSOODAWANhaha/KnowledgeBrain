import React from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { App } from "./App";
import "./app.css";

ReactDOM.createRoot(document.getElementById("app")!).render(
  <React.StrictMode>
    <App />
    <Toaster
      position="top-center"
      duration={4000}
      toastOptions={{
        className:
          "rounded-[12px] border border-[#ededf1] bg-white text-[#14141a] shadow-[0_1px_2px_rgba(17,17,26,.04),0_6px_18px_rgba(17,17,26,.05)]",
      }}
    />
  </React.StrictMode>,
);
