/** Host-specific defaults keep workspace paths and chat pins on their original host. */
export function hostPreferenceKey(key: string): string {
  const scope =
    typeof localStorage === "undefined"
      ? null
      : localStorage.getItem("amarcode-connection-scope");
  return scope ? `${key}:${scope}` : key;
}
