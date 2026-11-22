import React from "react";
import ReactDOM from "react-dom/client";
import { onEngine } from "./api";
import App from "./App";
import { applyEvent, loadInitial } from "./store";
import "./styles.css";

// Subscribe before loading so no event between the snapshot and the listener is lost.
onEngine(applyEvent).then(loadInitial);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
