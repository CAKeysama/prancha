import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
// Fontes da interface, todas embutidas (ver styles/index.css). O navegador só carrega a que estiver em uso.
import "@fontsource-variable/hanken-grotesk/wght.css";
import "@fontsource-variable/atkinson-hyperlegible-next/wght.css";
import "@fontsource-variable/ibm-plex-sans/wght.css";
import "@fontsource/ibm-plex-mono/400.css";
import "./styles/index.css";
import { App } from "./app/App";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
