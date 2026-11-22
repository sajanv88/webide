// Public entry of @repo/tabs: the open documents of a workspace, and their tabs.
export type { CloseDialogProps } from "./close-dialog";
export type { DocumentNoticeProps } from "./document-notice";
export {
  DocumentTabs,
  type DocumentTabsProps,
  type EditorSlotHandle,
  type EditorSlotProps,
} from "./document-tabs";
export {
  type CloseChoice,
  createDocuments,
  type DiskState,
  type DocumentContent,
  type DocumentId,
  type Documents,
  type DocumentsOptions,
  type DocumentsState,
  type DocumentView,
  type PendingClose,
} from "./documents";
export { endingAfterSave, toDiskText, toEditorText } from "./line-endings";
