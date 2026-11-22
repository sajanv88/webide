/**
 * Whether git takes `name` as a branch name, for the rules people meet (the
 * backend checks again with the same rules): no spaces or `~^:?*[\`, no `..`,
 * `@{` or `//`, not starting with `-` or `/`, not ending with `/`, `.` or
 * `.lock`, and no part starting with `.`.
 */
export function validBranchName(name: string): boolean {
  if (name === "" || name === "@") return false;
  if (name.startsWith("-") || name.startsWith("/") || name.endsWith("/")) return false;
  if (name.endsWith(".") || name.endsWith(".lock")) return false;
  if (name.includes("..") || name.includes("@{") || name.includes("//")) return false;
  if (name.split("/").some((part) => part === "" || part.startsWith("."))) return false;
  // biome-ignore lint/suspicious/noControlCharactersInRegex: control characters are what it rejects
  return !/[\s~^:?*[\\\u0000-\u001f\u007f]/.test(name);
}
