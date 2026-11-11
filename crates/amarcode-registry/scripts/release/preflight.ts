import { registryDirectory, run, wranglerConfig } from "./commands";

export function requireAuthenticatedWrangler(stdout: string): void {
  let identity: { loggedIn?: boolean };
  try {
    identity = JSON.parse(stdout);
  } catch {
    throw new Error(
      "Unable to verify Wrangler authentication. Run `bunx wrangler login` and retry.",
    );
  }
  if (identity?.loggedIn !== true) {
    throw new Error(
      "Wrangler is not authenticated. Run `bunx wrangler login` and retry.",
    );
  }
}

export function checkWranglerAuthentication(): void {
  // The human-readable whoami can exit successfully when logged out. JSON mode
  // exposes loggedIn and exits unsuccessfully for missing authentication.
  // Failures throw with the fix; success stays silent so the release prompt
  // is the first thing printed.
  const result = run(
    ["bunx", "wrangler", "whoami", "--json", "--config", wranglerConfig],
    {
      cwd: registryDirectory,
      quiet: true,
      allowFailure: true,
    },
  );
  if (!result.success) {
    throw new Error(
      "Wrangler authentication check failed. Run `bunx wrangler login` and retry before publishing.",
    );
  }
  requireAuthenticatedWrangler(result.stdout?.toString() ?? "");
}
