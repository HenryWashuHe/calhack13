import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, host: "127.0.0.1" },
  test: { environment: "node" },
} as Parameters<typeof defineConfig>[0]);
