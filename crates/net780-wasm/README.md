# net780-wasm

`net780` (NET780 デジタコ生データ ZIP パーサー、`../net780`) をブラウザから直接呼べる
wasm-bindgen ラッパー。`ippoan/fc1200-wasm` と同じ規約:

```bash
wasm-pack build --target web
```

`pkg/` (gitignore 済み) に npm パッケージが出力される。consumer (`ohishi-exp/nuxt-dtako-admin`)
からは `file:../dtako-scraper/crates/net780-wasm/pkg` として参照する (fc1200-wasm を
`file:../fc1200-wasm/pkg` で参照するのと同じパターン)。

## API

```ts
import init, { parse_net780_zip } from "net780-wasm"

await init()
const result = parse_net780_zip(new Uint8Array(zipArrayBuffer))
// result: { header, inf, distance_total_m, speed[], gps[], events[], warnings[] }
```

- ZIP 内の `.inf` / `.spd` / `.dsd` / `.gpd` / `.evd` を拡張子で探し、それぞれ 1 個ずつ
  含まれる前提でパースする (`docs/net780-binary-format.md` 参照、`../net780` が SoT)。
- 見つからない/パース失敗したファイルは fatal にせず `warnings` に理由を積んで
  部分的な結果を返す (1 ファイル壊れていても他の結果は見られるようにする)。
- 共通ヘッダ (`header`) は `.dsd` を優先し、無ければ他のバイナリファイルから読む
  (どのバイナリファイルの先頭にも同じ 256 byte ヘッダが入っている)。

## なぜ WASM か (TypeScript 再実装ではなく)

フォーマットのリバースエンジニアリング結果は `net780` (Rust) 1 箇所にだけ実装し、
ロジックの二重管理を避けるため。ZIP 展開も Rust 側 (`zip` crate、pure Rust の
`miniz_oxide` バックエンド) で完結させるので、consumer 側に JS zip ライブラリは不要。
