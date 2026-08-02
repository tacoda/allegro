import OpenAI from "openai";

// A normalized chat turn, decoupling the agent loop from the SDK shape and
// letting tests swap in a fake backend.
export interface ChatParams {
  model: string;
  temperature: number;
  messages: any[];
  tools?: any[];
}

export interface ToolCall {
  id: string;
  name: string;
  args: any;
}

export interface ChatResult {
  message: any; // the raw assistant message, appended back verbatim
  content: string;
  toolCalls: ToolCall[];
}

export type ChatBackend = (params: ChatParams) => Promise<ChatResult>;

let backend: ChatBackend | null = null;

// Tests call this to intercept model calls; pass null to restore the real one.
export function setChatBackend(fn: ChatBackend | null): void {
  backend = fn;
}

export function defaultModel(): string {
  return process.env.MODEL ?? "gpt-4o-mini";
}

let client: OpenAI | null = null;

function openai(): OpenAI {
  if (!client) {
    const apiKey = process.env.OPENAI_API_KEY;
    if (!apiKey) throw new Error("OPENAI_API_KEY is not set in the environment");
    client = new OpenAI({ apiKey });
  }
  return client;
}

export async function chat(params: ChatParams): Promise<ChatResult> {
  if (backend) return backend(params);

  const res = await openai().chat.completions.create({
    model: params.model,
    temperature: params.temperature,
    messages: params.messages,
    ...(params.tools?.length ? { tools: params.tools, tool_choice: "auto" } : {}),
  });
  const message = res.choices[0]?.message ?? { role: "assistant", content: "" };
  const toolCalls: ToolCall[] = (message.tool_calls ?? []).map((tc: any) => ({
    id: tc.id,
    name: tc.function.name,
    args: safeParse(tc.function.arguments),
  }));
  return { message, content: message.content ?? "", toolCalls };
}

function safeParse(s: string): any {
  try {
    return JSON.parse(s);
  } catch {
    return { input: s };
  }
}
