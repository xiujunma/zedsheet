# Using zedsheet in a React app

zedsheet compiles to a WebAssembly module that you load and `mount` into a DOM
element. Below is the full path from this repo to a working React component.

## 1. Build the npm package

```sh
# one-time setup
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# from the zedsheet repo root — produces ./pkg (ESM + .wasm + .d.ts)
wasm-pack build --target web --out-dir pkg --out-name zedsheet
```

`pkg/` is a self-contained, importable package:

```
pkg/
├── package.json      # name: "zedsheet"
├── zedsheet.js       # ESM: default export = init(), named export = mount()
├── zedsheet.d.ts     # TypeScript types
└── zedsheet_bg.wasm
```

## 2. Add it to your React project

Install the local package (or copy `pkg/` in and import by relative path):

```sh
npm install /absolute/path/to/zedsheet/pkg
```

The renderer also needs two static assets from this repo:

- **Styles** — copy `src/index.css` into your app and import it once
  (e.g. `import "./zedsheet.css"`).
- **Icon sprite** — copy this repo's `asset/` folder into your app's `public/`
  so the toolbar icons resolve at `/asset/sprite.svg` (the CSS references that
  absolute path).

```sh
cp /path/to/zedsheet/src/index.css   src/zedsheet.css
cp -R /path/to/zedsheet/asset         public/asset
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
import "./zedsheet.css";          // the copied styles
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
