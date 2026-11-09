import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { GameCover } from "./GameCover.tsx";

const game = (cover: string | null, header: string | null) => ({
  id: "epic-x",
  title: "Rocket League",
  artwork: {
    cover: { url: cover, alt: "cover" },
    header: { url: header, alt: "header" },
    screenshots: [],
  },
});

describe("GameCover", () => {
  it("uses the header for wide views", () => {
    render(<GameCover game={game("https://img/cover.jpg", "https://img/header.jpg")} kind="header" />);
    expect(screen.getByRole("img").getAttribute("src")).toBe("https://img/header.jpg");
  });

  it("falls back to the cover when a game has no header", () => {
    render(<GameCover game={game("https://img/cover.jpg", null)} kind="header" />);
    expect(screen.getByRole("img").getAttribute("src")).toBe("https://img/cover.jpg");
  });

  it("shows the placeholder without any artwork", () => {
    render(<GameCover game={game(null, null)} kind="header" />);
    expect(screen.getByRole("img").tagName).toBe("DIV");
  });
});
