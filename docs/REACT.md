# Using zedsheet in a React app

zedsheet compiles to a WebAssembly module that you load and `mount` into a DOM
element. Below is the full path from this repo to a working React component.

## 1. Install

```sh
npm install zedsheet
```

The package is self-contained:

```
zedsheet/
├── package.json      # name: "zedsheet"
├── zedsheet.js       # ESM: default export = init(), named exports = mount(), …
├── zedsheet.d.ts     # TypeScript types
├── zedsheet.css      # grid + chrome styles, toolbar icon sprite inlined
└── zedsheet_bg.wasm
```

To build it from source instead:

```sh
# one-time setup
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# from the zedsheet repo root — produces ./pkg
node scripts/build-npm.mjs
npm install /absolute/path/to/zedsheet/pkg
```

## 2. Import the stylesheet

Import it once, anywhere in your app — no assets to copy (the icon sprite is
inlined as a data URI):

```ts
import "zedsheet/zedsheet.css";
```

## 3. The React component

Copy [`examples/react/ZedSheet.tsx`](../examples/react/ZedSheet.tsx) into your
app. It dynamically imports the wasm module, initializes it once, and mounts a
spreadsheet into a `<div>` with a stable id (the wasm `mount` API takes a CSS
selector — the same selector also targets the JS data API). Props: `id`,
`data` (seed workbook), and `onChange` (fires with the serialized workbook
after every edit).

## 4. Use it

```tsx
import "zedsheet/zedsheet.css";   // grid + toolbar styles
import ZedSheet from "./ZedSheet";

export default function App() {
  // The container must have a real size — the grid sizes to its client box.
  return (
    <div style={{ height: "100vh", width: "100vw" }}>
      <ZedSheet />
    </div>
  );
}
```

That renders a fully interactive spreadsheet (toolbar, formula bar, editing,
formulas, multi-sheet tabs, …).

## API

```ts
import init, {
  mount,                              // mount(selector, dataJson?)
  get_data, load_data,                // workbook JSON: snapshot / restore
  on_change,                          // change callback (autosave, sync, …)
  export_csv, import_csv,             // active sheet ⇄ CSV text
  export_xlsx, import_xlsx,           // whole workbook ⇄ .xlsx bytes
  setSheetReadOnly, isSheetReadOnly,  // lock / query a sheet by name
} from "zedsheet";

await init();                              // load the wasm (call once)
mount("#my-grid");                         // blank sheet (pass JSON to seed)
on_change("#my-grid", (json) => { ... });  // fires after every edit
```

Every function takes the mounted container's CSS selector, except
`setSheetReadOnly` / `isSheetReadOnly`, which take a sheet name. `mount`
accepts x-spreadsheet-format JSON (a single sheet object or an array of
sheets); `get_data` returns the same format.

## Bundler notes

- **Vite** — works out of the box (ESM + async wasm via `init()`).
- **Next.js** — render client-side only: `const ZedSheet = dynamic(() => import("./ZedSheet"), { ssr: false })`.
- **CRA / webpack 5** — the `--target web` build shown here initializes the wasm
  manually (`await init()`), so no special webpack wasm config is required.
