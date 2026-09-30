import { describe, expect, it } from "vitest";
import { modeOf, surfaceFor, type Mode } from "./theme";
import { session } from "../test/fixtures";

const MODES: Mode[] = ["permission", "input", "running", "idle", "empty"];

describe("modeOf", () => {
  it("reads an empty list as empty", () => {
    expect(modeOf([])).toBe("empty");
  });

  it("treats a present but unremarkable session as idle", () => {
    expect(modeOf([session({ state: "idle" })])).toBe("idle");
    expect(modeOf([session({ state: "shell" })])).toBe("idle");
  });

  it("treats activity it cannot classify as running", () => {
    // Both mean "working": one reported, one inferred from recency.
    expect(modeOf([session({ state: "running" })])).toBe("running");
    expect(modeOf([session({ state: "active-unknown" })])).toBe("running");
  });

  it("escalates to input when something is waiting for a reply", () => {
    expect(modeOf([session({ state: "awaiting-input" })])).toBe("input");
  });

  it("escalates past input to permission, the only state that blocks work", () => {
    expect(modeOf([session({ state: "awaiting-permission" })])).toBe("permission");
  });

  it("picks the most urgent state in a mixed list, not the first", () => {
    // The pill has one surface and one ring, so it has to show the worst thing
    // in the list rather than whichever row happened to sort first.
    expect(
      modeOf([
        session({ state: "idle" }),
        session({ state: "running" }),
        session({ state: "awaiting-input" }),
      ]),
    ).toBe("input");
    expect(
      modeOf([
        session({ state: "awaiting-input" }),
        session({ state: "awaiting-permission" }),
        session({ state: "running" }),
      ]),
    ).toBe("permission");
    expect(
      modeOf([session({ state: "idle" }), session({ state: "running" })]),
    ).toBe("running");
  });

  it("does not count an ended session as something the user can act on", () => {
    // History is display-only by construction; a row that says "ended" must
    // never be what puts a ring around the pill.
    expect(modeOf([session({ state: "ended" })])).toBe("idle");
  });
});

describe("surfaceFor", () => {
  it("fills every field of the surface for every mode, bar the ring", () => {
    // `ring` is legitimately empty for the three quiet modes - see below.
    for (const mode of MODES) {
      const surface = surfaceFor(mode);
      for (const [field, value] of Object.entries(surface)) {
        if (field === "ring") continue;
        expect(value, `${mode}.${field}`).toBeTruthy();
      }
    }
  });

  it("rings the pill only when something is waiting on the user", () => {
    // The keyline is the loudest signal available, and it is spent on the two
    // states that need an answer - never on work in progress.
    expect(surfaceFor("permission").ring).toBeTruthy();
    expect(surfaceFor("input").ring).toBeTruthy();
    expect(surfaceFor("running").ring).toBe("");
    expect(surfaceFor("idle").ring).toBe("");
    expect(surfaceFor("empty").ring).toBe("");
  });

  it("spends more ink on permission than on input", () => {
    // A request for approval is the solid stamp; a request for input is the
    // hollow one. Reading down a column tells you the urgency without hue.
    expect(surfaceFor("permission").ring).not.toBe(surfaceFor("input").ring);
  });

  it("keeps the shell, title, sub and gloss identical across modes", () => {
    // Only the edge, the ring and the accent carry state. If the paper itself
    // changed per mode, "same paper, second leaf" would stop being true.
    const base = surfaceFor("idle");
    for (const mode of MODES) {
      const surface = surfaceFor(mode);
      expect(surface.shell, mode).toBe(base.shell);
      expect(surface.title, mode).toBe(base.title);
      expect(surface.sub, mode).toBe(base.sub);
      expect(surface.gloss, mode).toBe(base.gloss);
    }
  });

  it("steps the edge down from busy to quiet", () => {
    // Density, not hue: permission and input share full ink, then it fades.
    const edge = (mode: Mode) => surfaceFor(mode).edge;
    expect(edge("permission")).toBe(edge("input"));
    expect(edge("permission")).not.toBe(edge("running"));
    expect(edge("running")).not.toBe(edge("idle"));
    expect(edge("idle")).not.toBe(edge("empty"));
  });
});
