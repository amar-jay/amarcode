import { join } from "node:path";
import { projectRoot } from "./commands";

export type BinaryProductId = "daemon" | "acp";
export type ProductId = BinaryProductId | "app";

export type BinaryProduct = {
  id: BinaryProductId;
  label: string;
  crate: string;
  bin: string;
  cargoToml: string;
  cargoLockName: string;
  objectPrefix: string;
  urlPrefix: string;
  envBucket: string;
  defaultBucket: string;
  includeProtocolVersion: boolean;
  verifyLifecycleCli: boolean;
};

export const binaryProducts: Record<BinaryProductId, BinaryProduct> = {
  daemon: {
    id: "daemon",
    label: "amarcode-daemon",
    crate: "amarcode-daemon",
    bin: "amarcode-daemon",
    cargoToml: join(projectRoot, "crates", "daemon", "Cargo.toml"),
    cargoLockName: "amarcode-daemon",
    objectPrefix: "daemon",
    urlPrefix: "/v1/daemon",
    envBucket: "AMARCODE_DAEMON_BUCKET",
    defaultBucket: "amarcode-daemons",
    includeProtocolVersion: true,
    verifyLifecycleCli: true,
  },
  acp: {
    id: "acp",
    label: "amarcode-acp",
    crate: "amarcode-acp",
    bin: "amarcode-acp",
    cargoToml: join(projectRoot, "crates", "amarcode-acp", "Cargo.toml"),
    cargoLockName: "amarcode-acp",
    objectPrefix: "acp",
    urlPrefix: "/v1/acp",
    envBucket: "AMARCODE_ACP_BUCKET",
    defaultBucket: "amarcode-daemons",
    includeProtocolVersion: false,
    verifyLifecycleCli: false,
  },
};

export const allProductIds: ProductId[] = ["daemon", "acp", "app"];
export const binaryProductIds: BinaryProductId[] = ["daemon", "acp"];

export function parseProductId(value: string): ProductId {
  if (value === "daemon" || value === "acp" || value === "app") return value;
  throw new Error(`unknown product: ${value}`);
}

export function expandProducts(values: string[]): ProductId[] {
  const expanded = values.flatMap((value) =>
    value === "all" ? allProductIds : [parseProductId(value)],
  );
  return [...new Set(expanded)];
}

export function releaseCommitMessage(
  ids: BinaryProductId[],
  versions: Record<BinaryProductId, string>,
): string {
  const parts = ids.map((id) => `${binaryProducts[id].label} ${versions[id]}`);
  return `chore: release ${parts.join(", ")}`;
}
