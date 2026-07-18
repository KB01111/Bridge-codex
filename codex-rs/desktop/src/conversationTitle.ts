const MAX_CONVERSATION_TITLE_LENGTH = 52;
const MAX_CONVERSATION_TITLE_WORDS = 8;

export function conversationTitleFromPrompt(prompt?: string | null): string {
  if (!prompt?.trim()) {
    return "New task";
  }

  const normalized = prompt.trim().replace(/\s+/gu, " ");
  const words = normalized.split(" ");
  const wordBounded = words.slice(0, MAX_CONVERSATION_TITLE_WORDS).join(" ");
  const title =
    words.length > MAX_CONVERSATION_TITLE_WORDS
      ? `${wordBounded}…`
      : wordBounded;

  return title.length > MAX_CONVERSATION_TITLE_LENGTH
    ? `${title.slice(0, MAX_CONVERSATION_TITLE_LENGTH - 1).trimEnd()}…`
    : title;
}

export function conversationTitle(
  chat: Array<{ role: string; content: string }>,
): string {
  return conversationTitleFromPrompt(
    chat.find((message) => message.role === "user")?.content,
  );
}
