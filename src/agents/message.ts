// An agent's output — a structured value, not a bare string. Prints as its
// content, so logging a Message shows the text.
export class Message {
  constructor(
    public content: string,
    public role: string = "assistant",
    public from: string = "agent",
  ) {}

  get text(): string {
    return this.content;
  }

  get length(): number {
    return this.content.length;
  }

  toString(): string {
    return this.content;
  }
}
