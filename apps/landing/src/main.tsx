import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "@fontsource-variable/manrope/index.css"
import "@fontsource/instrument-serif/400.css"
import "./index.css"
import { App } from "./App"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
