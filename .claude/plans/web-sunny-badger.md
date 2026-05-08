# Fix mobile (iOS Safari) touch selection on chess-web board

## Context

Web 版的 chess-web 在桌面瀏覽器以滑鼠操作一切正常，但在 iOS Safari（手機版）
上**無法選中棋子**。從使用者提供的截圖可見 iOS 原生的文字選取選單
（"Copy / Find Selection / Look Up"）出現在棋盤上，並且最右側的 `俥` 周圍
出現了藍色的文字選取拖曳手柄 — 這是 iOS 對 SVG `<text>` 元素長按時的預設
行為，會搶走 touch 事件、阻止它被當成 `click` 派發到 hit-test 矩形上。

### 為什麼桌面 OK、手機壞？

1. **桌面**：滑鼠 click 事件直接命中 `clients/chess-web/src/components/board.rs:255` 的
   `<rect class="cell-hit" on:click=…>`，遊戲邏輯收到選擇。
2. **iOS Safari**：觸控落在 SVG 上時，瀏覽器先用文字選取啟發式分析觸控；只要使用者
   稍微按住或移動，iOS 就把觸控解讀為「選取文字」（棋子的中文字 `俥/帥/…`
   是 SVG `<text>`），於是：
   - 出現 callout 選單（Copy / Look Up）
   - 顯示選取手柄
   - 該觸控不再以 `click` 事件派發

`style.css` 雖然已經對 `.tile-glyph` 與 `.river-text` 設定 `user-select: none`
（lines 398, 410），但這只擋住「真的選取到文字」的部分；iOS 的 callout 選單
是另一條路徑（`-webkit-touch-callout`），而 tap-to-click 的行為則由
`touch-action` 控制 — 這兩個屬性目前**完全沒有設定**。

## Root cause (一行版)

`clients/chess-web/style.css` 的 `.board` 與 `.cell-hit` 缺少 iOS Safari
所需的 `-webkit-touch-callout: none`、`touch-action: manipulation`，以及
SVG 容器層級的 `user-select: none` — 導致觸控被當成文字選取手勢、`click`
事件無法到達 hit-test 圖層。

## Fix

**只需要改一個檔案**：`clients/chess-web/style.css`。

### 1. 在 `.board` 規則（line 384-389）加入觸控抑制屬性

把整個 SVG 容器標為「不可選取、不可長按、tap 直接視為 click」。所有子元素
（`<text>`、`<rect>`、`<circle>`）都會繼承 `user-select` 與
`-webkit-touch-callout`。

```css
.board {
    display: block;
    width: 100%;
    height: auto;
    max-height: 80vh;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
    touch-action: manipulation;
}
```

### 2. 在 `.cell-hit` 規則（line 453）顯式宣告 `touch-action`

雖然從 `.board` 繼承，但 `touch-action` 在某些 WebKit 版本上不會繼承到
個別 `<rect>`，所以保險起見在 hit-test 矩形上明寫一次。同時拿掉桌面才有
意義的 `:hover` fill 在觸控裝置上的「黏住」效果。

```css
.cell-hit {
    fill: transparent;
    cursor: pointer;
    touch-action: manipulation;
}
.cell-hit:hover { fill: rgba(212, 165, 92, 0.18); }
```

### 為什麼這四個屬性各自不可少

| 屬性 | 作用 |
|---|---|
| `user-select: none` / `-webkit-user-select: none` | 擋掉「選取到 SVG `<text>` 內容」這條路徑 |
| `-webkit-touch-callout: none` | 擋掉 iOS 長按時跳出的 Copy / Look Up callout 選單（與 `user-select` 是**獨立**機制） |
| `touch-action: manipulation` | 告訴瀏覽器這塊區域不會用到 double-tap-zoom、pan 等手勢；觸控直接派發為 `click`，並消除 300ms tap delay |

### 不需要改 `board.rs`

目前的 `on:click` handler 在桌面與行動裝置上都會正確 fire — 前提是觸控事件
沒被 iOS 文字選取攔截。CSS 修好後，現有的 `on:click=move |_| on_click.call(sq)`
（`board.rs:255`）會在手機上正常觸發。**不需要**新增 `on:pointerdown` /
`on:touchstart` — 多寫只會讓事件去重變複雜。

### 不需要動 viewport meta tag

`index.html:5` 的 `<meta name="viewport" content="width=device-width, initial-scale=1"/>`
已經是現代行動裝置的正確配置。**不要**加 `user-scalable=no`，那會破壞無障礙
（弱視使用者需要 pinch-zoom）。

## Files to modify

- `clients/chess-web/style.css` — 兩個 CSS 規則，共加 4 行 + 改 0 行刪 0 行

## Verification

1. **本機 dev server**：
   ```bash
   make play-web   # 啟動 chess-net + Trunk hot-reload
   ```
   或只跑 web client：
   ```bash
   trunk serve clients/chess-web/index.html
   ```

2. **桌面回歸測試**（Chrome / Firefox / Safari）：
   - 點擊棋子 → 應出現黃色選取框與綠色合法走步指示
   - hover `.cell-hit` → 仍出現淡黃色 hover 效果（`:hover` 規則保留）

3. **iOS Safari 手機測試**（GitHub Pages 部署 `daviddwlee84.github.io` 或本機 +
   ngrok）：
   - 短點擊棋子 → 立即選中（無 300ms 延遲）
   - **長按棋子 → 不應跳出 Copy / Look Up 選單，不應出現藍色拖曳手柄**
   - pinch-zoom 整頁仍可運作（沒擋掉縮放）

4. **Android Chrome 手機**：同上，行為應與 iOS 一致。

5. **Sanity build**：
   ```bash
   cargo build --target wasm32-unknown-unknown -p chess-web
   make build-web   # trunk build --release
   ```
   純 CSS 改動不會影響 Rust 編譯，但跑一次確認沒打錯。

## Out of scope (記錄一下，這次不做)

- **加 `on:pointerdown` 加速反應**：tap delay 已被 `touch-action: manipulation`
  解掉，再加 pointer event 會雙重觸發要去重，得不償失。
- **加 `on:touchstart` 處理拖曳**：本專案是 click-to-select / click-to-move
  介面，不是 drag-and-drop，不需要。
- **自訂 long-press 手勢**：目前沒有任何長按功能需求。
- **viewport `user-scalable=no`**：傷害無障礙，且不是這個 bug 的原因。
