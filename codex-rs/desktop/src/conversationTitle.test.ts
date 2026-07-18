import { describe, expect, it } from "vitest";

import {
  conversationTitle,
  conversationTitleFromPrompt,
} from "./conversationTitle";

describe("conversation titles", () => {
  it("normalizes whitespace and caps the title at eight words", () => {
    expect(
      conversationTitleFromPrompt(
        "  Analyze\nwhat\twe have completed and create a production plan  ",
      ),
    ).toBe("Analyze what we have completed and create a…");
  });

  it("caps unusually long prompts and handles an empty conversation", () => {
    expect(conversationTitleFromPrompt("x".repeat(80))).toHaveLength(52);
    expect(conversationTitle([])).toBe("New task");
  });
});
