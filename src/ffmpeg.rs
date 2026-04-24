//! ffmpeg 呼び出し（pad + subtitles 焼き込み）。

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

pub struct BurnInParams<'a> {
    pub panel_width: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub fonts_dir: Option<&'a Path>,
    pub crf: u8,
    pub preset: &'a str,
}

/// pad で左に黒帯を足し、ASS を焼き込む。
pub fn run_ffmpeg_burn_in(
    input: &Path,
    output: &Path,
    ass_path: &Path,
    p: BurnInParams<'_>,
) -> Result<()> {
    let w_out = p.panel_width + p.video_width;
    let h_out = p.video_height;
    let x_pad = p.panel_width;

    let ass_abs = ass_path
        .canonicalize()
        .unwrap_or_else(|_| ass_path.to_path_buf());
    let ass_str = ass_abs.to_string_lossy();

    // subtitles フィルタのパス: 特殊文字をエスケープ（Windows 含む一般的な対策として単引用符で囲む）
    let sub_filter = if ass_str.contains('\'') {
        anyhow::bail!("ASS パスに単引用符を含めないでください: {}", ass_str);
    } else {
        format!("subtitles='{}':charenc=UTF-8", ass_str)
    };

    let fonts_clause = if let Some(dir) = p.fonts_dir {
        let d = dir.to_string_lossy();
        if d.contains('\'') {
            anyhow::bail!("fonts-dir に単引用符を含めないでください");
        }
        format!(":fontsdir='{}'", d)
    } else {
        String::new()
    };

    let vf = format!(
        "pad={w_out}:{h_out}:{x_pad}:0:black,{}{}",
        sub_filter, fonts_clause
    );

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .args(["-vf", &vf])
        .args(["-c:v", "libx264", "-preset", p.preset, "-crf", &p.crf.to_string()])
        .args(["-c:a", "copy"])
        .arg(output)
        .status()
        .context("ffmpeg を起動できません。PATH に ffmpeg があるか確認してください")?;

    if !status.success() {
        anyhow::bail!("ffmpeg が非ゼロ終了しました（出力先・フィルタ・コーデックを確認）");
    }

    Ok(())
}
