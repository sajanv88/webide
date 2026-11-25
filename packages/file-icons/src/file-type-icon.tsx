import { FileIcon } from "@nuvui/icons";
import {
  CFileIcon,
  CppFileIcon,
  CsFileIcon,
  CssFileIcon,
  EnvFileIcon,
  type FileTypeIconComponent,
  GoFileIcon,
  HtmlFileIcon,
  JavaFileIcon,
  JsFileIcon,
  JsonFileIcon,
  JsxFileIcon,
  MdFileIcon,
  PhpFileIcon,
  PyFileIcon,
  RbFileIcon,
  RsFileIcon,
  ScssFileIcon,
  ShFileIcon,
  SqlFileIcon,
  TsFileIcon,
  TsxFileIcon,
  VueFileIcon,
  XmlFileIcon,
  YmlFileIcon,
} from "@nuvui/icons/file-types";

/** nuvui's icon for each extension, with the other spellings of the same type. */
const BY_EXTENSION: ReadonlyMap<string, FileTypeIconComponent> = new Map([
  ["ts", TsFileIcon],
  ["mts", TsFileIcon],
  ["cts", TsFileIcon],
  ["tsx", TsxFileIcon],
  ["js", JsFileIcon],
  ["mjs", JsFileIcon],
  ["cjs", JsFileIcon],
  ["jsx", JsxFileIcon],
  ["json", JsonFileIcon],
  ["jsonc", JsonFileIcon],
  ["json5", JsonFileIcon],
  ["html", HtmlFileIcon],
  ["htm", HtmlFileIcon],
  ["css", CssFileIcon],
  ["scss", ScssFileIcon],
  ["md", MdFileIcon],
  ["markdown", MdFileIcon],
  ["py", PyFileIcon],
  ["pyi", PyFileIcon],
  ["go", GoFileIcon],
  ["java", JavaFileIcon],
  ["rs", RsFileIcon],
  ["c", CFileIcon],
  ["h", CFileIcon],
  ["cpp", CppFileIcon],
  ["cc", CppFileIcon],
  ["cxx", CppFileIcon],
  ["hpp", CppFileIcon],
  ["hh", CppFileIcon],
  ["hxx", CppFileIcon],
  ["cs", CsFileIcon],
  ["php", PhpFileIcon],
  ["rb", RbFileIcon],
  ["sh", ShFileIcon],
  ["bash", ShFileIcon],
  ["zsh", ShFileIcon],
  ["sql", SqlFileIcon],
  ["yml", YmlFileIcon],
  ["yaml", YmlFileIcon],
  ["xml", XmlFileIcon],
  ["vue", VueFileIcon],
  ["env", EnvFileIcon],
]);

/**
 * The extension that picks a file's icon, in lower case, or `undefined` when
 * none of nuvui's file type icons is for it. `.env` and its variants
 * (`.env.local`, `.env.production`) are `env`.
 */
export function fileTypeOf(name: string): string | undefined {
  const lower = name.toLowerCase();
  if (lower === ".env" || lower.startsWith(".env.")) return "env";
  const dot = lower.lastIndexOf(".");
  if (dot < 0) return undefined;
  const extension = lower.slice(dot + 1);
  return BY_EXTENSION.has(extension) ? extension : undefined;
}

export interface FileTypeIconProps {
  /** The file's name, or its path: only the part after the last dot counts. */
  readonly name: string;
  /** Width and height in pixels. */
  readonly size?: number;
  readonly className?: string;
}

/**
 * A file's icon by its type: nuvui's file type icon (a page with the
 * extension on a colored tag) where there is one, its plain file icon
 * otherwise. Hidden from screen readers, as it stands beside the file's name.
 */
export function FileTypeIcon({ name, size = 16, className }: FileTypeIconProps) {
  const type = fileTypeOf(name);
  const Icon = type === undefined ? undefined : BY_EXTENSION.get(type);
  const props = {
    size,
    "aria-hidden": true,
    "data-file-type": type ?? "file",
    ...(className === undefined ? {} : { className }),
  } as const;
  return Icon ? <Icon {...props} /> : <FileIcon {...props} />;
}
