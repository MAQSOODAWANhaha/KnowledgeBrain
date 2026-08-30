import React from "react";
import ReactDOM from "react-dom/client";
import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import { theme } from "./theme";
import { App } from "./App";
import "./app.css";

ReactDOM.createRoot(document.getElementById("app")!).render(
  <React.StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="light">
      <Notifications position="top-center" autoClose={4000} />
      <App />
    </MantineProvider>
  </React.StrictMode>,
);
