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
spreadsheet into a ref'd `<div>` (which is given a unique id because the wasm
`mount` API takes a CSS selector).

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
import init, { mount } from "zedsheet";

await init();                  // load the wasm (call once)
mount(selector: string,        // CSS selector of the container
      data_json?: string);     // optional seed data (zedsheet JSON)
```

> **Data import/export is still in progress.** `mount` accepts zedsheet's own
> serialized JSON; full x-spreadsheet-format import and a `getData()` /
> `onChange()` API are tracked in issues #15 and #20. For now the most reliable
> use is a blank, interactive sheet (`mount("#id")`).

## Bundler notes

- **Vite** — works out of the box (ESM + async wasm via `init()`).
- **Next.js** — render client-side only: `const ZedSheet = dynamic(() => import("./ZedSheet"), { ssr: false })`.
- **CRA / webpack 5** — the `--target web` build shown here initializes the wasm
  manually (`await init()`), so no special webpack wasm config is required.
