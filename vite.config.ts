import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import { avatarkitVitePlugin } from "@spatius/avatarkit/vite";

export default defineConfig({
  plugins: [react(), avatarkitVitePlugin()],
  server: {
    port: 5174,
    proxy: { "/api": "http://localhost:5002" },
  },
});
