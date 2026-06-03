import { useEffect, useRef } from "react";

// Load + instantiate the .wasm once for the whole app.
let ready: Promise<typeof import("zedsheet")> | null = null;
function load() {
  if (!ready) {
    ready = import("zedsheet").then(async (mod) => {
      await mod.default(); // init(): fetches and instantiates the .wasm
      return mod;
    });
  }
  return ready;
}

type Props = {
  /** Container id — `#${id}` is also the selector the JS data API targets.
   *  Give each instance its own id. (Avoid `zedsheet`: that id triggers the
   *  standalone demo's auto-mount.) */
  id?: string;
  /** Seed workbook: zedsheet JSON (string, object, or array). Omit for blank. */
  data?: unknown;
  /** Called with the serialized workbook after every edit. */
  onChange?: (json: string) => void;
};

/** Interactive spreadsheet that fills its container (which must have a size). */
export default function ZedSheet({ id = "zedsheet-root", data, onChange }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    load().then((mod) => {
      if (cancelled || !ref.current) return;
      const seed =
        typeof data === "string" ? data : data ? JSON.stringify(data) : undefined;
      mod.mount(`#${id}`, seed);
      if (onChange) mod.on_change(`#${id}`, onChange);
    });
    return () => {
      cancelled = true;
    };
    // Mount once — the sheet owns its state from here on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  return <div id={id} ref={ref} style={{ width: "100%", height: "100%" }} />;
}
