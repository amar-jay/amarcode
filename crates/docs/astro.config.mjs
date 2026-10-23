import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

const site = new URL(process.env.SITE_URL || "https://amarcode.amarjay.com");
if (!["https:", "http:"].includes(site.protocol)) {
  throw new Error("SITE_URL must be an absolute HTTP(S) URL");
}
const socialImage = new URL("/docs/social-preview.png", site).href;
const socialImageAlt =
  "Amarcode: Your agents. One continuous conversation. Light and dark desktop workspaces against a misty mountain landscape.";

export default defineConfig({
  site: site.origin,
  base: "/docs",
  trailingSlash: "always",
  integrations: [
    starlight({
      title: "Amarcode",
      description:
        "Guides for your Amarcode workspace, coding agents, and remote daemon.",
      customCss: ["@fontsource-variable/geist", "./src/styles/amarcode.css"],
      favicon: "/favicon.ico",
      head: [
        {
          tag: "link",
          attrs: {
            rel: "icon",
            type: "image/svg+xml",
            href: "/docs/favicon.svg",
          },
        },
        {
          tag: "link",
          attrs: {
            rel: "apple-touch-icon",
            sizes: "180x180",
            href: "/docs/apple-touch-icon.png",
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
        SiteTitle: "./src/components/SiteTitle.astro",
        Head: "./src/components/Head.astro",
      },
      social: [
        {
          icon: "github",
          label: "Source on GitHub",
          href: "https://github.com/amar-jay/amarcode",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/amar-jay/amarcode/edit/main/crates/docs/",
      },
      sidebar: [
        { label: "Overview", slug: "index" },
        {
          label: "Getting started",
          items: [
            { label: "Install Amarcode", slug: "getting-started/installation" },
            {
              label: "Your first conversation",
              slug: "getting-started/first-conversation",
            },
          ],
        },
        {
          label: "Agents",
          items: [
            { label: "Choose and install an agent", slug: "agents/overview" },
            { label: "Configure a provider", slug: "agents/provider" },
            { label: "Permissions", slug: "agents/permissions" },
          ],
        },
        {
          label: "Your workspace",
          items: [
            {
              label: "Conversations and commands",
              slug: "workspace/conversations",
            },
            { label: "Files and Git", slug: "workspace/files-and-git" },
            {
              label: "Attachments and dictation",
              slug: "workspace/attachments-and-dictation",
            },
          ],
        },
        {
          label: "Remote daemon",
          items: [
            { label: "Set up a remote host", slug: "remote/setup" },
            { label: "Connect from the desktop", slug: "remote/connect" },
            {
              label: "Updates and troubleshooting",
              slug: "remote/troubleshooting",
            },
          ],
        },
        {
          label: "Understand Amarcode",
          items: [
            {
              label: "Desktop, daemon, and agents",
              slug: "concepts/architecture",
            },
            { label: "Conversation continuity", slug: "concepts/continuity" },
          ],
        },
        {
          label: "Contributing",
          items: [
            { label: "Development setup", slug: "contributing/development" },
            { label: "Protocol and testing", slug: "contributing/protocol" },
          ],
        },
      ],
    }),
  ],
});
