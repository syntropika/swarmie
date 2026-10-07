import { expect, it } from "vitest";
import { questionAnswers } from "./questions";
it("combines multiple choices with a written answer without changing its portable type", () => {
  expect(
    questionAnswers(
      [
        {
          id: "q",
          prompt: "Choose",
          options: [],
          multiple: true,
          allowText: true,
        },
      ],
      { q: ["a", "b"] },
      { q: "Another option" },
    ),
  ).toEqual({ q: ["a", "b", "Another option"] });
});
it("preserves single choice and secret free text answer shapes", () => {
  expect(
    questionAnswers(
      [
        { id: "a", prompt: "Choose", options: [] },
        { id: "secret", prompt: "Password", options: [], secret: true },
      ],
      { a: "first", secret: "value" },
      { a: "stale input" },
    ),
  ).toEqual({ a: "first", secret: "value" });
});
