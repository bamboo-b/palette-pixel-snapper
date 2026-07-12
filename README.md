# Palette Pixel Snapper

**ガタガタなドット絵を、綺麗なグリッドに整え直して、好きなパレットで塗り直すツール。**

AI生成のドット絵や、グリッドがズレてしまったドット絵を入力すると、

- ピクセルを**きっちり等間隔のグリッド**にスナップし直し、
- 色を**指定したパレット**（または自動生成パレット）に量子化して、

整った状態で出力します。

<img src="./static/demo.png" alt="AI生成画像 → グリッドにスナップ → パレット適用" style="width: 100%; image-rendering: pixelated;">

<p align="center"><em>左：AI生成の入力画像　→　中央：グリッドにスナップして整地　→　右：PICO-8パレットを適用</em></p>

> このツールは [Hugo Duprez](https://www.hugoduprez.com/) 氏の [Sprite Fusion Pixel Snapper](https://github.com/Hugo-Dz/spritefusion-pixel-snapper) の**フォーク**です。本家の「グリッドスナップ」機能に、**任意パレットの適用**（`.hex` / `.gpl` / `.pal` / 画像からの抽出、OKLabによる知覚的な色マッチング、ディザリング、パレット編集GUI）を追加しています。

---

## 何ができる？

| 入力 | このツール | 出力 |
|------|-----------|------|
| AI生成やガタガタのドット絵 | グリッド自動検出 → スナップ | ピクセルが等間隔に整列 |
| バラバラな色 | パレット量子化（OKLab） | 指定した色だけで構成 |
| 大きい/ボケた画像 | ダウンサンプリング | 1ドット=1ピクセルの綺麗な結果 |

**向いている用途**：AI生成ドット絵の仕上げ / タイルマップやアイソメ絵の整地 / パレットを固定したい2Dゲームアセット。

> ⚠️ このツールは**後処理（仕上げ）専用**です。ゼロからドット絵を「生成」する機能はありません。既存の画像を整えるツールです。

---

## セットアップ

[Rust](https://www.rust-lang.org/) が必要です。ブラウザGUIを使う場合は [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) も必要です。

```bash
git clone https://github.com/bamboo-b/palette-pixel-snapper.git
cd palette-pixel-snapper
```

> このフォークにはホスティング版はありません。下記の手順でローカルにビルドして使います。

---

## 使い方①：ブラウザGUI

<img src="./static/gui.png" alt="ブラウザGUIの操作画面" style="width: 100%;">

<p align="center"><em>画像を読み込み、パレットを指定して before / after を確認しながら書き出せます</em></p>

WASMをビルドしてから、`web/` フォルダをHTTPで配信します（ESモジュール＋wasmは `file://` から読めないため）。

```bash
# 1. WASMをビルド
wasm-pack build --target web --out-dir web/pkg --release

# 2. web/ を配信
cd web
python -m http.server 8000
# → ブラウザで http://localhost:8000/ を開く
```

### GUIでの操作

1. **画像をドラッグ&ドロップ**（またはファイル選択）
2. 必要なら**パレットを指定**（下記）
3. 「Limit colors」で使う色数を絞る（任意）
4. before/after を見比べて、**PNGでダウンロード**

パレット周りの機能：

- **パレット入力** — hexコードをテキストエリアに貼り付け、または Lospec `.hex` / GIMP `.gpl` / JASC `.pal` ファイルを読み込み。画像（`.png` / `.jpg`）を読み込んでその色を抽出することも可能（65色以上ある場合はk-meansで64色に自動縮小）。
- **スウォッチ ON/OFF** — 各色をクリックで使う/使わないを切り替え（「使う色」カウンター表示）。テキストエリアを編集してもON/OFF状態は保持され、有効な色だけを `.hex` に書き出せます。
- **コンセプトプリセット** — ワンクリックで条件に合う色だけ残すフィルタ：色相（暖色/寒色）、明度（明るい/暗い）、彩度（鮮やか/くすみ）、色相関係（補色/類似/三色）。判定はOKLCh（知覚的な色相・彩度・明度）で行います。パレットが全部暖色/全部寒色のときは、暖色/寒色は「より暖かい半分／より冷たい半分」に分割してくれます。関係プリセットはベース色を軸に動作：スウォッチを **Shift+クリック** で選ぶ（緑枠）か、最も彩度の高い色が自動的にベースになります。「すべてON」で全色を戻せます。
- **Seed + 🎲** — k-meansの色をランダムに引き直して別の組み合わせを試す。seedが効くとき（「Limit colors」ON、またはパレット未指定）だけ有効になります（純粋な最近傍スナップは決定論的なので無効）。
- **Dither** — 有効パレットに対してFloyd–Steinbergディザリングを適用（出力解像度で適用）。
- **プレビュー** — before/after比較に加え、**実寸1:1**の結果表示。

---

## 使い方②：CLI

### 基本

```bash
# 色数はデフォルト16
cargo run input.png output.png

# 色数を指定（第3引数 = k）
cargo run input.png output.png 16

# ディレクトリを渡すとバッチ処理（rayonで並列）
cargo run sprites/batch_inputs sprites/batch_outputs 16
```

### グリッドサイズを手動指定

自動検出が期待とズレるときは `--pixel-size` で上書き（1〜画像短辺の半分の範囲）。

```bash
cargo run input.png output.png --pixel-size 8
cargo run sprites/batch_inputs sprites/batch_outputs 16 --pixel-size 8
```

### バリエーションを試す（seed）

k-meansはシード固定なのでデフォルトは毎回同じ結果。`--seed` で代表色の引き直しができます（色数やパレットと併用時に効く）。

```bash
cargo run input.png output.png 16 --seed 7
cargo run input.png output.png --palette pico8.hex 8 --seed 7
```

---

## 使い方③：パレットを適用する

`--palette` で固定パレットを強制します。**色数を付けない**とk-meansをスキップし、全ピクセルを最も近いパレット色にスナップします。

```bash
# インラインでhexをカンマ区切り指定（# は省略可）
cargo run input.png output.png --palette "#1a1a2e,#16213e,0f3460,#e94560"

# Lospec .hex ファイル（1行1色）
cargo run input.png output.png --palette pico8.hex
cargo run sprites/batch_inputs sprites/batch_outputs --palette pico8.hex
```

**パレット＋色数の併用**：まず画像をk-meansでN色に減らし、その各色をパレットの最も近い色にスナップします。大きなパレット（例：64色）から自然な色味を保ちつつ色数を絞りたいときに便利。

```bash
cargo run input.png output.png --palette resurrect-64.hex 16
```

**対応フォーマット**：Lospec `.hex` に加え、GIMP `.gpl` / JASC `.pal`、さらに**画像**（`.png` / `.jpg`）を渡すとその色を抽出（65色以上なら64色にk-means縮小）。

```bash
cargo run input.png output.png --palette retro.gpl
cargo run input.png output.png --palette some_reference_art.png
```

**ディザリング**：`--dither` でFloyd–Steinbergディザを適用（グリッド確定後、出力解像度で適用）。少ない色数でもグラデーションを残しやすくなります。

```bash
cargo run input.png output.png --palette pico8.hex --dither
```

補足：

- 色マッチングは**知覚的**：sRGBの生の距離ではなく **OKLab** 空間で比較するので、見た目に近い色に揃います。
- `#` は省略可、3桁（`#abc`）・6桁（`#aabbcc`）どちらもOK。
- `.hex` ファイル内の空行と `;` コメント行は無視されます。
- パレット指定＋色数なし＝全パレット色が使用可能。色数ありだと最大その色数まで。
- 最大256色。単一画像・バッチ両対応。

---

## 使い方④：WASM API（開発者向け）

ビルド後、生成モジュールを読み込んで使います。

```js
import init, { process_image, extract_palette } from "./pkg/spritefusion_pixel_snapper.js";

await init();

// process_image(inputBytes, kColors?, pixelSizeOverride?, paletteRgb?, seed?, dither?)
const outputBytes = process_image(inputBytes, 16);
```

- 使わない省略可能引数には `undefined`（または `null`）を渡します。
- `paletteRgb`：RGB三つ組をフラットに並べた `Uint8Array`（`[r,g,b, r,g,b, ...]`、最大256色）。渡すとそのパレットにスナップ。`kColors` を併用すると「k-meansでN色に減らしてからパレットにスナップ」に切り替わります。
- `seed`（`u32`、省略可）：k-means初期化のシード。色数が絡む場合、同じ画像/パレットでも別の色の組み合わせになります。純粋な最近傍スナップには影響しません。デフォルト `42`。
- `dither`（`boolean`、省略可）：出力解像度でFloyd–Steinbergディザを適用。デフォルト `false`。
- `extract_palette(inputBytes, maxColors?)`：画像からパレットを抽出（不透明な一意色を、`maxColors` 超なら決定論的にk-means縮小。デフォルト64・最大256）。`process_image` の `paletteRgb` にそのまま渡せる `Uint8Array` を返します。

---

## パラメータ早見表（CLI）

| 引数 | 意味 | 例 |
|------|------|-----|
| `<input>` | 入力画像 or ディレクトリ（必須） | `in.png` |
| `<output>` | 出力先（必須） | `out.png` |
| `[k]` | 色数（位置引数・省略時16） | `16` |
| `--pixel-size <n>` | グリッド間隔を手動指定 | `--pixel-size 8` |
| `--palette <値>` | 固定パレット（インラインhex / `.hex` / `.gpl` / `.pal` / 画像） | `--palette pico8.hex` |
| `--seed <n>` | k-meansのシード | `--seed 7` |
| `--dither` | Floyd–Steinbergディザ適用 | `--dither` |

---

## 謝辞

本ツールは [Sprite Fusion](https://spritefusion.com) の **Pixel Snapper**（作者：Hugo Duprez）のフォークです。Sprite Fusion は Unity・Godot・Defold・GB Studio など多くのエンジンに対応した、無料のWebベース・タイルマップエディタです。本家ツール（ホスティング版・有料デスクトップ版を含む）は [spritefusion.com/pixel-snapper](https://www.spritefusion.com/pixel-snapper) にあります。

<img src="./static/spritefusion.webp" alt="Sprite Fusion" style="width: 100%;">

## ライセンス

MIT License — [Hugo Duprez](https://www.hugoduprez.com/)
