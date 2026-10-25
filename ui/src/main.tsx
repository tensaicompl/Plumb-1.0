import "./styles/fonts";
import "./styles/app.css";

import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { createQueryClient } from "./api/client";
import { router } from "./app/router";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("Plumb UI: the #root element is missing from index.html");
}

const queryClient = createQueryClient();

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
