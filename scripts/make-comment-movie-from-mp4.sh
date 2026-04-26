#!/usr/bin/env bash
#
# 概要: 入力の mp4（複数可）について、同じディレクトリの「<拡張子除いた名前>.comments.txt」
#       をコメント源として make-comment-movie を順に実行する。
# 仕様: 出力は同ディレクトリの「<拡張子除いた名前>.with-comments.mp4」。
#       先頭から連続する「存在する .mp4 パス」が対象。それ以外が現れたところから
#       make-comment-movie へ透過的に渡す。または `--` の後をすべて透過（複数 mp4
#       と併用可）。
# 制限: コメントファイル名は WhoWatch ログの想定通り .comments.txt 接尾辞であること。
#       バイナリは環境変数 MAKE_COMMENT_MOVIE、または本リポジトリの target/release、
#       または PATH 上の make-comment-movie の順に解決する。

set -euo pipefail

scriptDir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repoRoot=$(cd -- "$scriptDir/.." && pwd)

# 引数を「対象 mp4 の配列」と「make-comment-movie へ付与する配列」に分割する。戻りはグローバル。
_splitArgs() {
  mp4PathsOut=()
  passThroughOut=()
  local -a _args=("$@")
  local _i=0
  while ((_i < ${#_args[@]})); do
    local _a=${_args[_i]}
    if [[ "$_a" == "--" ]]; then
      passThroughOut=("${_args[@]:_i+1}")
      return
    fi
    if [[ -f "$_a" ]]; then
      local _bn
      _bn=$(basename -- "$_a")
      if [[ "$_bn" == *.mp4 ]]; then
        mp4PathsOut+=("$_a")
        ((_i++)) || true
        continue
      fi
      passThroughOut=("${_args[@]:_i}")
      return
    fi
    local _bn2
    _bn2=$(basename -- "$_a")
    if [[ "$_a" == *.mp4 || "$_bn2" == *.mp4 ]]; then
      echo "エラー: 入力の mp4 が存在しません: $_a" >&2
      exit 1
    fi
    passThroughOut=("${_args[@]:_i}")
    return
  done
}

_splitArgs "$@"

if ((${#mp4PathsOut[@]} == 0)); then
  echo "使い方: $0 <入力.mp4> [入力2.mp4 ...] [make-comment-movie 向けオプション...]" >&2
  echo "  あるいは: $0 <入力.mp4> ... -- [同オプション...]" >&2
  echo "  例: $0 録画1.mp4 録画2.mp4" >&2
  echo "  例: $0 録画1.mp4 録画2.mp4 -- --crf 18" >&2
  exit 1
fi

if [[ -n "${MAKE_COMMENT_MOVIE:-}" ]]; then
  binPath=$MAKE_COMMENT_MOVIE
elif [[ -x "$repoRoot/target/release/make-comment-movie" ]]; then
  binPath=$repoRoot/target/release/make-comment-movie
elif command -v make-comment-movie >/dev/null 2>&1; then
  binPath=$(command -v make-comment-movie)
else
  echo "エラー: make-comment-movie を解決できません。次のいずれかを行ってください。" >&2
  echo "  - cargo build --release で $repoRoot/target/release/ を生成する" >&2
  echo "  - PATH に make-comment-movie を通す" >&2
  echo "  - MAKE_COMMENT_MOVIE=/path/to/make-comment-movie を指定する" >&2
  exit 1
fi

if [[ ! -x "$binPath" ]] && ! command -v "$binPath" >/dev/null 2>&1; then
  echo "エラー: 実行可能な make-comment-movie ではありません: $binPath" >&2
  exit 1
fi

anyFailure=0
for mp4Path in "${mp4PathsOut[@]}"; do
  mp4Name=$(basename -- "$mp4Path")
  baseName=${mp4Name%.mp4}
  mp4Dir=$(dirname -- "$mp4Path")
  commentsFile="$mp4Dir/${baseName}.comments.txt"
  outputPath="$mp4Dir/${baseName}.with-comments.mp4"

  if [[ ! -f "$commentsFile" ]]; then
    echo "エラー: 同じディレクトリにコメントファイルが見つかりません: $commentsFile" >&2
    echo "  対象動画: $mp4Path" >&2
    echo "  期待: <動画名と同じ接頭辞>.comments.txt" >&2
    anyFailure=1
    continue
  fi

  if ! "$binPath" --input "$mp4Path" --comments "$commentsFile" --output "$outputPath" \
    "${passThroughOut[@]}"; then
    echo "エラー: 処理に失敗しました: $mp4Path" >&2
    anyFailure=1
  fi
done

exit "$anyFailure"
