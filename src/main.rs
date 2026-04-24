//! WhoWatch 形式のコメントと録画 mp4 から、左に黒パネル＋白字コメントを焼き込んだ動画を生成する CLI。
//!
//! 主な仕様: ファイル名または `--video-start` で動画の t=0 時刻を決定し、`posted_at_local` と差分で同期する。
//! 制限: ffmpeg / ffprobe が PATH に必要。日本語フォントは環境依存のため `--font-name` / `--fonts-dir` で指定可能。

mod ass;
mod comments;
mod ffprobe;
mod ffmpeg;
mod video_start;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

/// コメントを左パネルに焼き込んだ mp4 を生成する（ffmpeg 再エンコード）。
#[derive(Parser, Debug)]
#[command(name = "make-comment-movie")]
struct Args {
    /// 入力動画（mp4 等 ffmpeg が読める形式）
    #[arg(short, long)]
    input: PathBuf,

    /// WhoWatch 形式のコメント TSV（.comments.txt 等）
    #[arg(short, long)]
    comments: PathBuf,

    /// 出力動画パス
    #[arg(short, long)]
    output: PathBuf,

    /// 動画の t=0 に対応する絶対時刻（例: 2026-04-12T14:51:31）。未指定時は入力ファイル名から推定
    #[arg(long)]
    video_start: Option<String>,

    /// 左パネル幅（ピクセル）。未指定時は映像高さの約 30%（下限・上限あり）
    #[arg(long)]
    panel_width: Option<u32>,

    /// パネルに残す過去コメントの最大経過秒（これより古い行はスタックから除外。大きいほど長く残る）
    #[arg(long, default_value_t = 600.0)]
    max_dwell_sec: f64,

    /// パネル内に積む最大行数（古い行が上、最新が下）
    #[arg(long, default_value_t = 10)]
    max_lines: u32,

    /// 新コメントが上に付くときの落下アニメ時間（ミリ秒・上揃えパネル）
    #[arg(long, default_value_t = 380)]
    scroll_ms: i64,

    /// ASS / ffmpeg で使うフォント名（システムにインストール済みであること）
    #[arg(long, default_value = "Noto Sans CJK JP")]
    font_name: String,

    /// フォントファイルが入ったディレクトリ（任意。未指定時は ffmpeg 既定のフォント解決に任せる）
    #[arg(long)]
    fonts_dir: Option<PathBuf>,

    /// 字幕の本文フォントサイズ（px 相当）
    #[arg(long, default_value_t = 22)]
    font_size: i32,

    /// 表示名（リスナー名）のフォントサイズ。未指定時は本文より約 2pt 小さい値を自動算出
    #[arg(long)]
    name_font_size: Option<i32>,

    /// 映像エンコードの CRF（小さいほど高品質）
    #[arg(long, default_value_t = 20)]
    crf: u8,

    /// libx264 preset（ultrafast 〜 veryslow）
    #[arg(long, default_value = "medium")]
    preset: String,

    /// 中間 ASS を残すパス（デバッグ用）。未指定時は一時ファイルを削除
    #[arg(long)]
    keep_ass: Option<PathBuf>,

    /// BY_PLAYITEM などギフト系コメントを除外する
    #[arg(long, default_value_t = false)]
    skip_playitem: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let video_start = video_start::resolve_video_start(&args.input, args.video_start.as_deref())
        .with_context(|| {
            format!(
                "動画の開始時刻を決定できませんでした。--video-start を指定するか、ファイル名に YYYY-MM-DDTHH-MM-SS を含めてください: {:?}",
                args.input
            )
        })?;

    let mut entries = comments::parse_comments_file(&args.comments, args.skip_playitem)
        .with_context(|| format!("コメントファイルの読み込みに失敗: {:?}", args.comments))?;

    comments::annotate_timeline(&mut entries, video_start);

    if entries.is_empty() {
        anyhow::bail!(
            "表示対象のコメントがありません（動画開始より前のみ、または --skip-playitem で全行除外された可能性）"
        );
    }

    let geom = ffprobe::probe_video_geometry(&args.input)
        .with_context(|| format!("ffprobe に失敗: {:?}", args.input))?;
    let duration_sec = ffprobe::probe_duration_seconds(&args.input)
        .with_context(|| format!("ffprobe duration に失敗: {:?}", args.input))?;

    let panel_w = args.panel_width.unwrap_or_else(|| ffprobe::default_panel_width(geom.height));

    let segments = comments::build_display_segments(&entries, duration_sec);
    if segments.is_empty() {
        anyhow::bail!("表示セグメントが生成できませんでした");
    }

    let ass_path = if let Some(ref p) = args.keep_ass {
        p.clone()
    } else {
        tempfile_ass_path()?
    };

    let name_font_size = args.name_font_size.unwrap_or_else(|| {
        (args.font_size - 5).clamp(10, args.font_size.saturating_sub(1).max(10))
    });

    ass::write_ass_file(
        &ass_path,
        &entries,
        &segments,
        ass::AssLayout {
            play_res_x: panel_w + geom.width,
            play_res_y: geom.height,
            panel_width: panel_w,
            font_name: &args.font_name,
            font_size: args.font_size,
            name_font_size,
            max_lines: args.max_lines,
            stack_retention_sec: args.max_dwell_sec,
            scroll_ms: args.scroll_ms,
        },
    )
    .with_context(|| format!("ASS 生成に失敗: {:?}", ass_path))?;

    ffmpeg::run_ffmpeg_burn_in(
        &args.input,
        &args.output,
        &ass_path,
        ffmpeg::BurnInParams {
            panel_width: panel_w,
            video_width: geom.width,
            video_height: geom.height,
            fonts_dir: args.fonts_dir.as_deref(),
            crf: args.crf,
            preset: &args.preset,
        },
    )
    .with_context(|| "ffmpeg 実行に失敗")?;

    if args.keep_ass.is_none() {
        let _ = fs::remove_file(&ass_path);
    }

    Ok(())
}

fn tempfile_ass_path() -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let name = format!(
        "make-comment-movie-{}.ass",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    Ok(dir.join(name))
}
