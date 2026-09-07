/**
 * Explicit drag handle.
 *
 * The whole pill is a drag region, but a frameless window gives no hint that
 * it can be moved at all - so say so with a grip the cursor can aim at.
 */
export function DragGrip() {
  return (
    <div
      data-tauri-drag-region
      title="Drag to move"
      className="flex h-full cursor-grab items-center pl-0.5 pr-1 active:cursor-grabbing"
    >
      <svg width="8" height="20" viewBox="0 0 8 20" aria-hidden="true">
        {[4, 10, 16].map((y) =>
          [1.5, 6.5].map((x) => (
            <circle key={`${x}-${y}`} cx={x} cy={y} r="1.1" className="fill-zinc-400 dark:fill-zinc-600" />
          )),
        )}
      </svg>
    </div>
  );
}
