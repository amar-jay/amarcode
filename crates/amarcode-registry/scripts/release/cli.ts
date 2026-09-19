import * as prompts from "@clack/prompts";
import {
  defaultTargets,
  gitBranch,
  gitCommit,
  gitStatusShort,
  hostTarget,
  run,
} from "./commands";
import { bump, packageVersion } from "./manifest";
import {
  binaryProductIds,
  binaryProducts,
  expandProducts,
  type BinaryProductId,
  type ProductId,
} from "./products";

export type AppPublicationPlan = {
  commitMessage: string | null;
  push: boolean;
};

export type ParsedOptions = {
  products: ProductId[];
  productsProvided: boolean;
  versions: Partial<Record<BinaryProductId, string>>;
  versionsProvided: Partial<Record<BinaryProductId, boolean>>;
  targets: string[];
  bucket: string | null;
  overwrite: boolean;
  skipBuild: boolean;
  skipDeploy: boolean;
  dryRun: boolean;
  allowDirty: boolean;
  nonInteractive: boolean;
  appPublication: AppPublicationPlan | null;
};

export type ReleasePlan = {
  products: ProductId[];
  versions: Record<BinaryProductId, string>;
  targets: string[];
  bucket: string;
  overwrite: boolean;
  skipBuild: boolean;
  skipDeploy: boolean;
  dryRun: boolean;
  allowDirty: boolean;
  appPublication: AppPublicationPlan | null;
};

function cancelled(value: unknown): null {
  if (prompts.isCancel(value)) prompts.cancel("Publication cancelled.");
  return null;
}

function printHelp() {
  console.log(`Usage: bun run publish [products] [options]

Products:
  daemon | acp | app | all

Options:
  --products <list>         Comma-separated products (daemon,acp,app,all)
  --version <version>       Version when a single binary product is selected
  --daemon-version <ver>    Daemon release version
  --acp-version <ver>       ACP release version
  --overwrite               Replace an existing remote version
  --target <triple>         Rust target; repeat for multiple targets
  --bucket <name>           R2 bucket (default: amarcode-daemons)
  --skip-build              Publish already-built release binaries
  --skip-deploy             Upload without deploying the registry Worker
  --dry-run                 Build and generate metadata without Cloudflare writes
  --allow-dirty             Allow production publish from a dirty worktree
  --non-interactive         Fail instead of opening the release TUI
  --commit-message <msg>    Commit remaining changes before app publication
  --push                    Push main and trigger the desktop release workflow
`);
}

export function parseArgs(
  args: string[],
  presetProducts: ProductId[] = [],
): ParsedOptions {
  const values = new Map<string, string>();
  const targets: string[] = [];
  const flags = new Set<string>();
  const positionals: string[] = [];

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (
      [
        "--overwrite",
        "--skip-build",
        "--skip-deploy",
        "--dry-run",
        "--allow-dirty",
        "--non-interactive",
        "--push",
      ].includes(argument)
    ) {
      flags.add(argument);
      continue;
    }
    if (
      [
        "--version",
        "--daemon-version",
        "--acp-version",
        "--target",
        "--bucket",
        "--products",
        "--commit-message",
      ].includes(argument)
    ) {
      const value = args[++index];
      if (!value || value.startsWith("--")) {
        throw new Error(`${argument} requires a value`);
      }
      if (argument === "--target") targets.push(value);
      else values.set(argument, value);
      continue;
    }
    if (argument === "--help" || argument === "-h") {
      printHelp();
      process.exit(0);
    }
    if (argument.startsWith("-")) {
      throw new Error(`unknown argument: ${argument}`);
    }
    positionals.push(argument);
  }

  const products = expandProducts([
    ...presetProducts,
    ...(values.get("--products")?.split(",") ?? []),
    ...positionals,
  ]);

  const versions: ParsedOptions["versions"] = {};
  const versionsProvided: ParsedOptions["versionsProvided"] = {};
  const sharedVersion = values.get("--version");
  if (sharedVersion) {
    if (products.filter((id) => id !== "app").length > 1) {
      throw new Error(
        "--version applies to a single binary product; use --daemon-version and --acp-version",
      );
    }
    const binary = products.find((id): id is BinaryProductId => id !== "app");
    if (binary) {
      versions[binary] = sharedVersion;
      versionsProvided[binary] = true;
    }
  }
  const daemonVersion = values.get("--daemon-version");
  if (daemonVersion) {
    versions.daemon = daemonVersion;
    versionsProvided.daemon = true;
  }
  const acpVersion = values.get("--acp-version");
  if (acpVersion) {
    versions.acp = acpVersion;
    versionsProvided.acp = true;
  }

  const commitMessage = values.get("--commit-message") ?? null;
  const appPublication =
    products.includes("app") && flags.has("--push")
      ? { commitMessage, push: true }
      : null;

  return {
    products,
    productsProvided: products.length > 0,
    versions,
    versionsProvided,
    targets: [...new Set(targets)],
    bucket: values.get("--bucket") ?? null,
    overwrite: flags.has("--overwrite"),
    skipBuild: flags.has("--skip-build"),
    skipDeploy: flags.has("--skip-deploy"),
    dryRun: flags.has("--dry-run"),
    allowDirty: flags.has("--allow-dirty"),
    nonInteractive: flags.has("--non-interactive"),
    appPublication,
  };
}

async function inquireVersion(
  product: BinaryProductId,
): Promise<{ version: string; overwriteHint: boolean } | null> {
  const current = packageVersion(binaryProducts[product]);
  const kind = await prompts.select({
    message: `${binaryProducts[product].label} version`,
    initialValue: "patch",
    options: [
      { value: "patch", label: "Patch", hint: bump(current, "patch") },
      { value: "minor", label: "Minor", hint: bump(current, "minor") },
      { value: "major", label: "Major", hint: bump(current, "major") },
      { value: "custom", label: "Custom version" },
      {
        value: "current",
        label: "Keep current",
        hint: "deliberate replacement",
      },
    ],
  });
  if (prompts.isCancel(kind)) return cancelled(kind);
  if (kind === "custom") {
    const custom = await prompts.text({
      message: "Custom release version",
      placeholder: current,
      validate: (value) =>
        /^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(value)
          ? undefined
          : "Enter a valid release version.",
    });
    if (prompts.isCancel(custom)) return cancelled(custom);
    return { version: custom, overwriteHint: true };
  }
  if (kind === "current") return { version: current, overwriteHint: true };
  return { version: bump(current, kind), overwriteHint: false };
}

async function inquireAppPublication(): Promise<
  AppPublicationPlan | null | undefined
> {
  const branch = gitBranch();
  if (branch !== "main") {
    throw new Error(
      `desktop publication requires the main branch; current branch is ${branch || "detached HEAD"}`,
    );
  }
  run(["gh", "--version"], { quiet: true });
  run(["gh", "auth", "status"], { quiet: true });

  const changes = gitStatusShort();
  let commitMessage: string | null = null;
  if (changes) {
    prompts.note(changes, "Changes to commit before publishing the app");
    const commitChanges = await prompts.confirm({
      message: "Commit all listed release changes before publishing the app?",
      initialValue: true,
    });
    if (prompts.isCancel(commitChanges))
      return cancelled(commitChanges) ?? undefined;
    if (!commitChanges) return null;
    const message = await prompts.text({
      message: "Commit message",
      initialValue: "chore: publish amarcode release",
      validate: (value) =>
        value.trim() ? undefined : "Enter a non-empty commit message.",
    });
    if (prompts.isCancel(message)) return cancelled(message) ?? undefined;
    commitMessage = message.trim();
  }

  const push = await prompts.confirm({
    message: "Push main to origin and trigger the desktop release workflow?",
    initialValue: true,
  });
  if (prompts.isCancel(push)) return cancelled(push) ?? undefined;
  if (!push) return null;
  return { commitMessage, push: true };
}

export async function interactive(
  options: ParsedOptions,
): Promise<ReleasePlan | null> {
  const tty = process.stdin.isTTY && process.stdout.isTTY;
  const binarySelected = options.products.filter(
    (id): id is BinaryProductId => id !== "app",
  );
  const missingVersions = binarySelected.filter(
    (id) => !options.versionsProvided[id],
  );
  const needsTui =
    !options.productsProvided ||
    missingVersions.length > 0 ||
    (options.products.includes("app") && !options.appPublication);

  if (!needsTui) {
    return finalize(options);
  }
  if (!tty || options.nonInteractive) {
    throw new Error(
      "no interactive terminal; supply --products and product versions, or omit --non-interactive",
    );
  }

  prompts.intro("Amarcode release");
  prompts.note(`Git commit: ${gitCommit()}`, "Release source");

  let products = options.products;
  if (!options.productsProvided) {
    const selected = await prompts.multiselect({
      message: "Products to publish",
      initialValues: ["daemon"],
      options: [
        { value: "daemon", label: "Daemon" },
        { value: "acp", label: "ACP adapter" },
        { value: "app", label: "Desktop app" },
      ],
      required: true,
    });
    if (prompts.isCancel(selected)) return cancelled(selected);
    products = selected as ProductId[];
  }

  const versions: Record<BinaryProductId, string> = {
    daemon:
      options.versions.daemon ?? packageVersion(binaryProducts.daemon),
    acp: options.versions.acp ?? packageVersion(binaryProducts.acp),
  };
  let overwrite = options.overwrite;
  for (const id of binaryProductIds) {
    if (!products.includes(id) || options.versionsProvided[id]) continue;
    const chosen = await inquireVersion(id);
    if (!chosen) return null;
    versions[id] = chosen.version;
    if (chosen.overwriteHint) {
      const replace = await prompts.confirm({
        message: `Replace ${binaryProducts[id].label} ${chosen.version} if it already exists?`,
        initialValue: false,
      });
      if (prompts.isCancel(replace)) return cancelled(replace);
      overwrite = overwrite || replace;
    }
  }

  let targets = options.targets;
  if (products.some((id) => id !== "app") && targets.length === 0) {
    const currentHost = hostTarget();
    const targetSet = await prompts.select({
      message: "Release targets",
      initialValue: "all",
      options: [
        { value: "all", label: "Linux + Windows", hint: "recommended" },
        { value: "host", label: "Current host", hint: currentHost },
      ],
    });
    if (prompts.isCancel(targetSet)) return cancelled(targetSet);
    targets =
      targetSet === "host" ? [currentHost] : [...defaultTargets];
  }

  let appPublication = options.appPublication;
  if (products.includes("app") && !appPublication) {
    const plan = await inquireAppPublication();
    if (plan === undefined) return null;
    if (plan === null) {
      products = products.filter((id) => id !== "app");
    } else {
      appPublication = plan;
    }
  }

  const planLines = [
    `Products: ${products.join(", ") || "(none)"}`,
    ...products
      .filter((id): id is BinaryProductId => id !== "app")
      .map((id) => `${binaryProducts[id].label}: ${versions[id]}`),
    ...(products.some((id) => id !== "app")
      ? [`Targets: ${targets.join(", ")}`]
      : []),
    `Overwrite: ${overwrite ? "yes" : "no"}`,
    `Dirty tree: ${options.allowDirty ? "allowed" : "refused for production"}`,
    `Desktop app: ${products.includes("app") ? "publish last" : "skip"}`,
    ...(appPublication?.commitMessage
      ? [`Commit: ${appPublication.commitMessage}`]
      : []),
    ...(options.dryRun ? ["Mode: dry-run"] : []),
  ];
  prompts.note(planLines.join("\n"), "Release plan");
  const confirmed = await prompts.confirm({
    message: "Build and publish this release?",
    initialValue: false,
  });
  if (prompts.isCancel(confirmed)) return cancelled(confirmed);
  if (!confirmed) {
    prompts.cancel("Publication cancelled.");
    return null;
  }
  prompts.log.success("Release confirmed. Preparing artifacts…");

  return finalize({
    ...options,
    products,
    versions,
    versionsProvided: { daemon: true, acp: true },
    targets,
    overwrite,
    appPublication,
  });
}

function finalize(options: ParsedOptions): ReleasePlan {
  if (options.products.length === 0) {
    throw new Error("select at least one product: daemon, acp, or app");
  }
  const versions: Record<BinaryProductId, string> = {
    daemon:
      options.versions.daemon ?? packageVersion(binaryProducts.daemon),
    acp: options.versions.acp ?? packageVersion(binaryProducts.acp),
  };
  for (const id of binaryProductIds) {
    if (options.products.includes(id) && !options.versionsProvided[id]) {
      throw new Error(
        `supply --${id}-version <version> for non-interactive ${id} publishing`,
      );
    }
  }
  const targets =
    options.targets.length > 0 ? options.targets : [...defaultTargets];
  const bucket =
    options.bucket ??
    process.env.AMARCODE_DAEMON_BUCKET ??
    process.env.AMARCODE_ACP_BUCKET ??
    "amarcode-daemons";
  return {
    products: options.products,
    versions,
    targets,
    bucket,
    overwrite: options.overwrite,
    skipBuild: options.skipBuild,
    skipDeploy: options.skipDeploy,
    dryRun: options.dryRun,
    allowDirty: options.allowDirty,
    appPublication: options.products.includes("app")
      ? options.appPublication
      : null,
  };
}

