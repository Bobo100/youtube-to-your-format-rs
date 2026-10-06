import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import App from "./App";

describe("App", () => {
  it("renders the window title in Traditional Chinese", () => {
    expect(renderToString(<App />)).toContain("YouTube 下載");
  });
});
