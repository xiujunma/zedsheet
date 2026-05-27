import { useEffect, useRef } from "react";

// Load + initialize the wasm module once for the whole app.
let ready: Promise<typeof import("zedsheet")> | null = null;
function load() {
  if (!ready) {
    ready = (async () => {
      const mod = await import("zedsheet");
      await mod.default(); // init(): fetches and instantiates the .wasm
      return mod;
    })();
  }
  return ready;
}

type Props = {
  /** Optional seed data (zedsheet JSON). Omit for a blank sheet. */
  data?: unknown;
  style?: React.CSSProperties;
  className?: string;
};

/**
 * Mounts an interactive zedsheet spreadsheet that fills its container.
 * The container must have a defined size (the canvas sizes to its client box).
 */
export default function ZedSheet({ data, style, className }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    const el = ref.current;
    if (!el) return;
    // The wasm `mount` takes a CSS selector, so give the container a stable id.
    if (!el.id) el.id = "zedsheet-" + Math.random().toString(36).slice(2);

    load().then((mod) => {
      if (cancelled) return;
      mod.mount("#" + el.id, data ? JSON.stringify(data) : undefined);
    });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      ref={ref}
      className={className}
      style={{ width: "100%", height: "100%", ...style }}
    />
  );
}
