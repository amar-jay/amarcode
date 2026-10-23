const url = new URL(process.env.SITE_URL || "https://amarcode.amarjay.com");
if (!["https:", "http:"].includes(url.protocol)) {
  throw new Error("SITE_URL must be an absolute HTTP(S) URL");
}

export const site = url.origin;
export const socialImage = new URL("/social-preview.png", site).href;
export const socialImageAlt =
  "Amarcode: Your agents. One continuous conversation. Light and dark desktop workspaces against a misty mountain landscape.";
