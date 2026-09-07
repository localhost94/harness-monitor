import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PANEL } from "./lib/theme";
import { Pill } from "./components/Pill";
import { SessionList } from "./components/SessionList";
import { useMonitor } from "./store/useMonitor";

export default function App() {
  const { init, expanded, theme, orientation } = useMonitor();
  const vertical = orientation === "vertical" && !expanded;

  useEffect(() => {
    void init();
  }, [init]);

  // Tauri's `data-tauri-drag-region` only matches the exact mousedown target,
  // so clicking the headline text - the biggest part of the pill - would not
  // drag. Handle it here instead: anywhere in the pill starts a drag, except
  // real controls. The session list is excluded so it stays scrollable.
  useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const target = event.target as HTMLElement | null;
      if (!target?.closest("[data-drag-zone]")) return;
      if (target.closest("button, a, input, [data-no-drag]")) return;
      void getCurrentWindow().startDragging();
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, []);

  return (
    <div className={`flex h-full flex-col overflow-hidden rounded-2xl ${theme === "dark" ? "dark" : ""}`}>
      {/* The vertical strip fills the window; the horizontal pill is fixed height. */}
      <div className={vertical ? "min-h-0 flex-1" : "flex-none"}>
        <Pill />
      </div>
      {expanded && (
        <div
          className={`relative mt-1 flex-1 overflow-hidden rounded-[20px] border pt-2 backdrop-blur-xl ${PANEL}`}
        >
          {/* Same hairline gloss as the pill, so panel and pill read as one object. */}
          <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-white/60 to-transparent dark:from-white/12" />
          <SessionList />
        </div>
      )}
    </div>
  );
}
