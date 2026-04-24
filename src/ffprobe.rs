//! ffprobe による映像ストリームの解像度取得。

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy)]
pub struct VideoGeometry {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
struct FfprobeRoot {
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

/// 最初の映像ストリームの幅・高さを返す。
pub fn probe_video_geometry(path: &Path) -> Result<VideoGeometry> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_type",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .context("ffprobe を起動できません。PATH に ffprobe があるか確認してください")?;

    if !out.status.success() {
        anyhow::bail!(
            "ffprobe が失敗しました: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let parsed: FfprobeRoot = serde_json::from_slice(&out.stdout).context("ffprobe JSON のパース")?;

    for s in parsed.streams {
        if s.codec_type.as_deref() == Some("video") {
            let w = s.width.context("width がありません")?;
            let h = s.height.context("height がありません")?;
            return Ok(VideoGeometry {
                width: w,
                height: h,
            });
        }
    }

    anyhow::bail!("映像ストリームが見つかりません: {:?}", path);
}

/// コンテナの再生時間（秒）。取得できない場合はエラーとする。
pub fn probe_duration_seconds(path: &Path) -> Result<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .context("ffprobe を起動できません")?;

    if !out.status.success() {
        anyhow::bail!(
            "ffprobe (duration) が失敗: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || s == "N/A" {
        anyhow::bail!("duration を取得できません: {:?}", path);
    }
    s.parse::<f64>()
        .with_context(|| format!("duration の数値化に失敗: {:?}", s))
}

/// 左パネル幅の既定値（映像高さの約 30%、200〜 560px にクランプ）。
pub fn default_panel_width(video_height: u32) -> u32 {
    let raw = (video_height as u64 * 30 / 100).max(200).min(560);
    raw as u32
}
