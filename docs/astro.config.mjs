// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightImageZoom from "starlight-image-zoom";
import starlightThemeFlexoki from "starlight-theme-flexoki";

// https://astro.build/config
export default defineConfig({
  // Public URL for eventual GitHub Pages hosting; harmless locally.
  site: "https://cybersader.github.io",
  base: "/portaconv",
  vite: {
    server: {
      // Vite 6+ blocks non-localhost Host headers by default. Opens it back up
      // for LAN / Tailscale / Docker previews. Safe for local dev only.
      allowedHosts: true,
    },
  },
  integrations: [
    starlight({
      title: "portaconv",
      description:
        "Terminal-native conversation extractor + MCP server for agent CLIs.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/cybersader/portaconv",
        },
      ],
      editLink: {
        baseUrl:
          "https://github.com/cybersader/portaconv/edit/main/docs/",
      },
      lastUpdated: true,
      plugins: [
        starlightThemeFlexoki(),
        starlightImageZoom(),
      ],
      sidebar: [
        {
          label: "Getting started",
          autogenerate: { directory: "getting-started" },
        },
        {
          label: "Concepts",
          autogenerate: { directory: "concepts" },
        },
        {
          label: "Reference",
          autogenerate: { directory: "reference" },
        },
        {
          label: "Project",
          autogenerate: { directory: "project" },
        },
      ],
    }),
  ],
});
