// The allegro library: a system is a graph. Agentic primitives are nodes; the
// spec layer composes them with transitions, dependencies, and triggers.
export * from "./agents/index.ts";
export * from "./spec/index.ts";
export { bus, Bus, type BusEvent } from "./runtime/bus.ts";
