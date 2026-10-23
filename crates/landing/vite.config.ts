import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    {
      name: "amarcode-social-metadata",
      transformIndexHtml(html) {
        const site = new URL(
          process.env.SITE_URL || "https://amarcode.amarjay.com",
        );
        if (!["https:", "http:"].includes(site.protocol)) {
          throw new Error("SITE_URL must be an absolute HTTP(S) URL");
        }
        site.search = "";
        site.hash = "";
        return html.replaceAll("__SITE_URL__", site.href.replace(/\/$/, ""));
      },
    },
  ],
  server: {
    proxy: {
      "/docs": { target: "http://localhost:4321", changeOrigin: true },
    },
  },
  preview: { proxy: {} },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
});
