// Shared test harness for the agent examples: a canned model backend so every
// example runs without a key. Mock when `--mock` (ALLEGRO_MOCK=1) is set OR when
// there is no OPENAI_API_KEY to use; otherwise the real model answers.
import { setChatBackend, type ChatBackend, type ChatResult } from "../src/index.ts";

export function mocking(): boolean {
  return process.env.ALLEGRO_MOCK === "1" || !process.env.OPENAI_API_KEY;
}

// Simple case: map the last user turn to a fixed reply (no tool calls).
export function mock(reply: (userText: string) => string): void {
  mockRaw(async (p) => say(reply(String((p.messages.at(-1) as any)?.content ?? ""))));
}

// Full control: install a backend that may emit tool calls. No-op with a real key.
export function mockRaw(backend: ChatBackend): void {
  if (mocking()) setChatBackend(backend);
}

// Result builders matching the chat backend shape.
export const say = (content: string): ChatResult => ({
  message: { role: "assistant", content },
  content,
  toolCalls: [],
});

export const callTool = (name: string, input: string, id = "1"): ChatResult => ({
  message: { role: "assistant", content: null },
  content: "",
  toolCalls: [{ id, name, args: { input } }],
});

// Did the conversation already run a tool (i.e. we're on the follow-up turn)?
export const sawTool = (messages: any[]): boolean => messages.some((m) => m.role === "tool");
export const lastToolOutput = (messages: any[]): string =>
  String([...messages].reverse().find((m) => m.role === "tool")?.content ?? "");
export const systemText = (messages: any[]): string => String(messages.find((m) => m.role === "system")?.content ?? "");
export const firstUser = (messages: any[]): string => String(messages.find((m) => m.role === "user")?.content ?? "");
