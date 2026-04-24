# make-comment-movie

WhoWatch（ふわっち）形式のコメントログ（`.comments.txt`）と画面録画の **mp4** から、**左側に黒パネルを追加**し、その上に**動画内時刻に同期したコメント**（**本文は白字、表示名は緑**）を **ffmpeg で再エンコード焼き込み**した動画を生成する CLI です。コメント本文は **Unicode 行区切り候補**（`unicode-linebreak`）と **表示幅**（`unicode-width`）に基づき、**`\N` で事前折り返し**します（1 行の列上限はパネル内の利用幅とフォントサイズから全角 ≈ 幅 2 に合わせ、右の空きが減るよう `COLS_FILL_RATIO` で上振れして算出）。ASS は **`WrapStyle: 2` と `\q2`** で自動折り返しを無効化し、手動改行と二重に折られたり欠けたりしないようにしています。**最新のコメントがパネル上端に来**、より古いコメントがその下に並びます（新しいコメントが増えると全体が下へ伸びます）。

## 前提

- **ffmpeg** と **ffprobe** が `PATH` で実行できること（動作確認例: Debian の ffmpeg 5.1）。
- 日本語表示には、システムの fontconfig が解決できるフォントが必要です。文字化けする場合は `--font-name` や `--fonts-dir` を指定してください。
- 左パネル内のコメントは **左 10px・映像側 1px** の非対称余白（`MarginL` / `MarginR`）で、左は広めのまま、右だけ詰めて折り返し幅を広げています。

## ビルド

```bash
env -u RUSTC_WRAPPER -u CARGO_BUILD_RUSTC_WRAPPER cargo build --release
```

実行ファイル: `target/release/make-comment-movie`

（環境によって `RUSTC_WRAPPER` が設定されビルドが失敗する場合は、上記のようにアンセットしてください。）

## 使い方

```bash
./target/release/make-comment-movie \
  --input ロンシン_2026-04-12T14-51-31.mp4 \
  --comments ロンシン_2026-04-12T14-51-31.comments.txt \
  --output ロンシン_2026-04-12T14-51-31.with-comments.mp4
```

（`PATH` に入れていない場合は `cargo run --release --` のあとに同じ引数を続けてもよい。）

### 動画の t=0 の時刻

既定では **入力 mp4 のファイル名**に含まれる `YYYY-MM-DDTHH-MM-SS`（例: `2026-04-12T14-51-31`）を、動画の先頭フレームに対応するローカル日時とみなします。

ファイル名から取れない場合は **`--video-start`** で明示します。

```bash
make-comment-movie -i ./rec.mp4 -c ./rec.comments.txt -o ./out.mp4 \
  --video-start "2026-04-12T14:51:31"
```

### 主なオプション

| オプション | 説明 |
|------------|------|
| `--panel-width` | 左パネル幅（px）。省略時は映像高さの約 30%（200〜560px にクランプ） |
| `--max-dwell-sec` | パネルに残す過去コメントの最大経過秒（古すぎる行をスタックから除外）既定: 600 |
| `--max-lines` | パネルに載せる最大コメント件数（上が新しい件）既定: 10 |
| `--scroll-ms` | 新コメントが上に付くときの落下アニメ（ミリ秒）既定: 380 |
| `--font-name` | ASS で使うフォント名。既定: `Noto Sans CJK JP` |
| `--fonts-dir` | フォントファイルを置いたディレクトリ（任意） |
| `--font-size` | 本文のフォントサイズ。既定: 22 |
| `--name-font-size` | 表示名のフォントサイズ。省略時は本文より約 5pt 小さい値（最小 10） |
| `--crf` / `--preset` | libx264 の品質・速度。既定: CRF 20, preset medium |
| `--skip-playitem` | `BY_PLAYITEM`（ギフト等）の行を除外 |
| `--keep-ass` | 生成した ASS を指定パスに残す（デバッグ用） |

音声は可能な限り **ストリームコピー**（`-c:a copy`）します。コンテナと音声コーデックの組み合わせで失敗する場合は、ffmpeg 側で音声の再エンコードが必要になることがあります。

### ffmpeg のログに `Glyph 0x…. not found` / `fontselect` が出るとき

- **意味**: コメントに含まれる文字のコードポイントが、**主フォント（既定: Noto Sans CJK JP）に無く**、かつ libass が見つけられる **フォールバック用フォントにも無い**ときに出ます。例として `0x13212` など **U+13000 台はエジプト象形文字**で、CJK 向けフォントには載っていないことが多いです。
- **多くの場合エンコード自体は完了**しますが、当該文字は **豆腐（□）や空白**になります。
- **対処**（どれか一つ、または併用）:
  1. **`--fonts-dir`** で、**複数の Noto 系（またはグリフの広いフォント）の `.ttf` / `.otf` / `.ttc` をまとめたディレクトリ**を渡す。libass は `fontsdir` 内の別フォントで欠けたグリフを補おうとします。
  2. OS に **補助フォント**を入れる（例: ディストリビューションの `fonts-noto` / `fonts-noto-extra` など、配布形態は環境依存）。**象形文字用**なら [Noto Sans Egyptian Hieroglyphs](https://fonts.google.com/noto/specimen/Noto+Sans+Egyptian+Hieroglyphs) を取得して `--fonts-dir` に置く方法が確実です。
  3. 欠けを許容するか、コメント側でその文字を避ける。

パスに**単引用符 `'`** が含まれると `--fonts-dir` がエラーになる実装なので、ディレクトリ名は避けてください（ツールの制限）。

## ファイル名の例

コメントファイルと同じ接頭辞で、動画に日時が含まれる想定です。

- 動画: `ロンシン_2026-04-12T14-51-31.mp4`
- コメント: `ロンシン_2026-04-12T14-51-31.comments.txt`

## 仕様の詳細

[SPECIFICATION.md](./SPECIFICATION.md) を参照してください。

## ライセンス

MIT OR Apache-2.0（`Cargo.toml` に準拠）
