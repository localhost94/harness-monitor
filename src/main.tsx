import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// Preview harness: Chrome enforces a ~500px minimum window width, so a
// screenshot at the pill's real 420px is impossible without constraining the
// root ourselves. ?w=420 does exactly that.
const previewParams = new URLSearchParams(location.search);
const previewWidth = previewParams.get("w");
const previewHeight = previewParams.get("h");
if (previewWidth || previewHeight) {
  const root = document.getElementById("root") as HTMLElement;
  if (previewWidth) root.style.width = `${previewWidth}px`;
  if (previewHeight) root.style.height = `${previewHeight}px`;
}
