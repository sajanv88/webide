// Public entry of @repo/contracts.
// Everything under ./generated is written by `pnpm contracts:generate` from the
// Rust DTOs in crates/ide-ipc. Never edit it by hand; `pnpm lint` fails if it drifts.
export type * from "./app";
export type * from "./generated/index";
