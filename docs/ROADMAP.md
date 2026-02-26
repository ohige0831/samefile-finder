# ROADMAP — SameFile_Finder (Rust)

最終更新: 2026-02-26  
現行: **v2.2.0**

## 方針
- まず **運用で効く土台（キャッシュ）** を固める
- 次に **UI polish → 判断補助 → 検証/同期（A/B比較）** の順で育てる
- 「賢い推測」は避け、**確実に当たる範囲だけ** を提案する（Keep候補など）

---

## v2.3.0（最優先）— グローバルDB + キャッシュ管理
### 目的
- 親/子フォルダなど対象を変えても **キャッシュを共有**して高速化
- DB分散をやめてメンテしやすくする

### 実装項目
1. **グローバルDB化**
   - DB保存先（Windows想定）: `%LOCALAPPDATA%\SameFileFinder\cache.sqlite3`
   - 既存のローカルDB方式（対象フォルダ配下 `.samefile_finder_cache.sqlite3`）は互換維持（扱いは検討）
2. **キャッシュ統計の表示/ログ**
   - Entries件数 / DBサイズ
   - fingerprint/hash の cache hit/miss（可能なら）
3. **掃除機能（GC）**
   - 存在しないパス削除（最優先）
   - 任意: 最終利用日（last_seen/last_used）で削除（例: N日未使用）
   - 任意: `VACUUM`（手動ボタン）
4. **DBロック耐性**
   - `busy_timeout`
   - WALモード（必要なら明示）

---

## v2.3.1（UI polish）— 見た目と閲覧体験の仕上げ
### 目的
- すでに良いUIを“さらに気持ちよく”する

### 実装項目
- ファイル行高さ調整（例: 46px）
- hash短縮表示 + hoverでフル表示
- related folders の **chips化**（多い場合 `+N more`）
- 検索に **file name only** トグル
- 除外拡張子 **プリセットボタン**
  - Media: `lrc, txt, cue, m3u, m3u8`
  - Photos: `xmp, thm, aae`
  - Dev: `log, tmp, cache`

---

## v2.3.2（判断補助）— Keep候補 + Reclaim
### 目的
- 「見る」から「決める」へ（過剰な推定はしない）

### 実装項目
1. **Keep候補（★）表示**
   - ルールは保守的に限定:
     - `copy`, `コピー`, `(1)`, `(2)` など“ありがちな複製名”のみを根拠にする
     - `AAA` vs `bbb` のような意味推定はしない
   - 同点時:
     - target配下優先 → パス短い方 → ファイル名昇順
2. **Reclaim表示**
   - グループ: `(n-1) * size`
   - フォルダ: 合算
   - 任意: ソートに `Reclaim desc` を追加

---

## v2.4.0（設定の永続化）— “育つツール”化
### 目的
- 毎回の手入力を減らして常用感を上げる

### 実装項目
- 前回値の保存
  - `exclude_extensions_input`
  - folder grouping / sort / filter / search query
  - （任意）target path
- ユーザー定義プリセットの保存（除外拡張子など）

---

## v3.0.0（将来の柱）— A/B Compare（バックアップ検証）
### 目的
- 「バックアップできたつもり」を潰す検証ツールへ

### Phase 1（Read-only）
- Path A / Path B を **相対パス基準**で比較
- 出力カテゴリ:
  - Missing in B（AにあるがBにない）
  - Different content（相対パス同一だが内容が違う）
  - Only in B（任意）
  - Matched（件数のみでOK）

### Phase 2（安全な修復）
- Missing in B のみを対象にコピー（不足分のみ）
- コピー後に再検証（Compare → Repair → Re-Compare）

### Phase 3（同期アシスト）
- Different の扱い（上書き/世代保存/確認必須）
- レポート出力（CSV/JSON）
- 検証ログ保管（YYYYMMDD など）

---

## Notes
- 仕様追加は「安全に当たる範囲」から導入する（特にRepair系）
- DB集約後はGCと統計表示をセットで実装して運用性を担保する