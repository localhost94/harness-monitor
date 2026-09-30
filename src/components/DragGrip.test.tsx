import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { DragGrip } from "./DragGrip";

describe("DragGrip", () => {
  it("is a drag region, because the whole pill is one and the grip must agree", () => {
    const { container } = render(<DragGrip />);
    expect(container.querySelector("[data-tauri-drag-region]")).not.toBeNull();
  });

  it("says it can be moved, since a frameless window gives no other hint", () => {
    render(<DragGrip />);
    expect(screen.getByTitle("Drag to move")).toBeInTheDocument();
  });

  it("offers a grab cursor rather than the default arrow", () => {
    const { container } = render(<DragGrip />);
    expect((container.firstElementChild as HTMLElement).className).toContain("cursor-grab");
  });

  it("draws six dots, and hides them from assistive tech", () => {
    // Purely decorative: the grip is a mouse affordance, and a screen reader
    // announcing "image" six times helps nobody.
    const { container } = render(<DragGrip />);
    expect(container.querySelectorAll("circle")).toHaveLength(6);
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });
});
