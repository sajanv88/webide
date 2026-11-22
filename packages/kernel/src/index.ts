// Public entry of @repo/kernel: how modules talk without importing each other.
export {
  type Command,
  type CommandRegistry,
  type CommandRegistryOptions,
  createCommandRegistry,
  type Dispose,
  type ExecuteResult,
} from "./command-registry";
export { createEventBus, type EventBus, type EventBusOptions, type Listener } from "./event-bus";
