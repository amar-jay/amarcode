import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { site, socialImage, socialImageAlt } from "./site.config.mjs";

export default defineConfig({
  site,
  trailingSlash: "always",
  vite: {
    plugins: [tailwindcss()],
    // Checks/builds must not replace optimized modules used by a live dev server.
    cacheDir: process.argv.includes("dev")
      ? "node_modules/.vite/astro-dev"
      : "node_modules/.vite/astro-tools",
  },
  integrations: [
    react(),
    starlight({
      title: "Amarcode",
      description:
        "Guides for your Amarcode workspace, coding agents, and remote daemon.",
      customCss: ["@fontsource-variable/geist", "./src/styles/docs.css"],
      favicon: "/favicon.ico",
      head: [
        {
          tag: "link",
          attrs: {
            rel: "icon",
            type: "image/svg+xml",
            href: "/favicon.svg",
          },
        },
        {
          tag: "link",
          attrs: {
            rel: "apple-touch-icon",
            sizes: "180x180",
            href: "/apple-touch-icon.png",
          },
        },
        {
          tag: "meta",
          attrs: { name: "application-name", content: "Amarcode Docs" },
        },
        {
          tag: "meta",
          attrs: {
            name: "theme-color",
            content: "#fafaf6",
            media: "(prefers-color-scheme: light)",
          },
        },
        {
          tag: "meta",
          attrs: {
            name: "theme-color",
            content: "#18221d",
            media: "(prefers-color-scheme: dark)",
          },
        },
        { tag: "meta", attrs: { property: "og:image", content: socialImage } },
        {
          tag: "meta",
          attrs: { property: "og:image:type", content: "image/png" },
        },
        { tag: "meta", attrs: { property: "og:image:width", content: "1200" } },
        { tag: "meta", attrs: { property: "og:image:height", content: "630" } },
        {
          tag: "meta",
          attrs: { property: "og:image:alt", content: socialImageAlt },
        },
        { tag: "meta", attrs: { name: "twitter:image", content: socialImage } },
        {
          tag: "meta",
          attrs: { name: "twitter:image:alt", content: socialImageAlt },
        },
      ],
      components: {
        SiteTitle: "./src/components/docs/SiteTitle.astro",
        Head: "./src/components/docs/Head.astro",
        PageTitle: "./src/components/docs/PageTitle.astro",
      },
      social: [
        {
          icon: "github",
          label: "Source on GitHub",
          href: "https://github.com/amar-jay/amarcode",
        },
      ],
      editLink: {
        baseUrl:
          "https://github.com/amar-jay/amarcode/edit/main/crates/landing/",
      },
      sidebar: [
        { label: "Overview", slug: "docs" },
        {
          label: "Getting started",
          items: [
            {
              label: "Install Amarcode",
              slug: "docs/getting-started/installation",
            },
            {
              label: "Your first conversation",
              slug: "docs/getting-started/first-conversation",
            },
          ],
        },
        {
          label: "Agents",
          items: [
            {
              label: "Choose and install an agent",
              slug: "docs/agents/overview",
            },
            { label: "Use your own provider", slug: "docs/agents/provider" },
            { label: "Permissions", slug: "docs/agents/permissions" },
          ],
        },
        {
          label: "Your workspace",
          items: [
            {
              label: "Conversations and commands",
              slug: "docs/workspace/conversations",
            },
            { label: "Files and Git", slug: "docs/workspace/files-and-git" },
            {
              label: "Attachments and dictation",
              slug: "docs/workspace/attachments-and-dictation",
            },
          ],
        },
        {
          label: "Remote daemon",
          items: [
            { label: "Get started", slug: "docs/remote/overview" },
            { label: "Keep it running", slug: "docs/remote/standalone" },
            { label: "Troubleshooting", slug: "docs/remote/troubleshooting" },
            {
              label: "Advanced",
              collapsed: true,
              items: [
                { label: "Host setup options", slug: "docs/remote/setup" },
                { label: "Connection reference", slug: "docs/remote/connect" },
                { label: "Remote terminal and SSH", slug: "docs/remote/ssh" },
                { label: "Encryption and trust", slug: "docs/remote/security" },
                {
                  label: "Certificates and direct TLS",
                  slug: "docs/remote/certificates",
                },
                {
                  label: "Paired end-to-end encryption",
                  slug: "docs/remote/end-to-end",
                },
                {
                  label: "Company policy and revocation",
                  slug: "docs/remote/administration",
                },
              ],
            },
          ],
        },
        {
          label: "Understand Amarcode",
          items: [
            {
              label: "Desktop, daemon, and agents",
              slug: "docs/concepts/architecture",
            },
            {
              label: "Conversation continuity",
              slug: "docs/concepts/continuity",
            },
          ],
        },
        {
          label: "Contributing",
          items: [
            {
              label: "Development setup",
              slug: "docs/contributing/development",
            },
            {
              label: "Protocol and testing",
              slug: "docs/contributing/protocol",
            },
          ],
        },
      ],
    }),
  ],
});
