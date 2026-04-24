//! 動画の t=0 に対応する絶対時刻の決定（ファイル名パースまたは CLI 上書き）。

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;

fn filename_datetime_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d{4})-(\d{2})-(\d{2})T(\d{2})-(\d{2})-(\d{2})")
            .expect("valid regex")
    })
}

/// `--video-start` 文字列をパースする。許容形式: `YYYY-MM-DDTHH:MM:SS` または `YYYY-MM-DD HH:MM:SS`（秒の小数は任意）。
pub fn parse_video_start_arg(s: &str) -> Result<NaiveDateTime> {
    let s = s.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(dt);
    }
    anyhow::bail!(
        "video_start の形式が不正です: {:?}（例: 2026-04-12T14:51:31 または 2026-04-12 14:51:31）",
        s
    );
}

/// 入力動画パスのファイル名から開始時刻を推定する（`YYYY-MM-DDTHH-MM-SS` を含む名前を想定）。
pub fn parse_video_start_from_path(video_path: &Path) -> Option<NaiveDateTime> {
    let stem = video_path.file_name()?.to_str()?;
    let re = filename_datetime_regex();
    let cap = re.captures(stem)?;
    let y: i32 = cap.get(1)?.as_str().parse().ok()?;
    let mo: u32 = cap.get(2)?.as_str().parse().ok()?;
    let d: u32 = cap.get(3)?.as_str().parse().ok()?;
    let h: u32 = cap.get(4)?.as_str().parse().ok()?;
    let mi: u32 = cap.get(5)?.as_str().parse().ok()?;
    let s: u32 = cap.get(6)?.as_str().parse().ok()?;
    let date = NaiveDate::from_ymd_opt(y, mo, d)?;
    date.and_hms_opt(h, mi, s)
}

/// CLI またはファイル名から動画開始の絶対時刻を返す。
pub fn resolve_video_start(video_path: &Path, override_str: Option<&str>) -> Result<NaiveDateTime> {
    if let Some(s) = override_str {
        return parse_video_start_arg(s).context("--video-start のパース");
    }
    parse_video_start_from_path(video_path)
        .with_context(|| format!("ファイル名から開始時刻を抽出できません: {:?}", video_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_filename_pattern() {
        let p = PathBuf::from("ロンシン_2026-04-12T14-51-31.mp4");
        let dt = parse_video_start_from_path(&p).unwrap();
        assert_eq!(dt.to_string(), "2026-04-12 14:51:31");
    }
}
