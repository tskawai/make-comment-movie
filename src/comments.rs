//! WhoWatch 形式コメント TSV の読み込みと、動画内秒・表示終了時刻の付与。

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;

/// 1 行分のコメント（表示用テキストとタイムライン）。
#[derive(Debug, Clone)]
pub struct CommentEntry {
    /// ログ上の投稿時刻
    pub posted_at: NaiveDateTime,
    /// 動画内の表示開始秒（t=0 が動画開始）
    pub start_sec: f64,
    /// 表示終了秒
    pub end_sec: f64,
    pub display_name: String,
    pub message: String,
    #[allow(dead_code)]
    pub comment_type: String,
}

fn parse_posted_at(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    None
}

/// コメントファイルを読み込み、投稿時刻順にソートしたエントリ一覧を返す（秒は未設定のまま）。
pub fn parse_comments_file(path: &Path, skip_playitem: bool) -> Result<Vec<CommentEntry>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {:?}", path))?;
    let mut with_dt: Vec<CommentEntry> = Vec::new();

    for line in raw.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("posted_at_local\t") {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        // WhoWatch ログは 6 列: posted_at_local, user_id, display_name, account_name, comment_type, message
        if cols.len() < 6 {
            continue;
        }
        let posted_at = cols[0];
        let display_name = cols[2].to_string();
        let comment_type = cols[4].to_string();
        let message = cols[5].to_string();

        if skip_playitem && comment_type == "BY_PLAYITEM" {
            continue;
        }

        let Some(dt) = parse_posted_at(posted_at) else {
            continue;
        };

        with_dt.push(CommentEntry {
            posted_at: dt,
            start_sec: 0.0,
            end_sec: 0.0,
            display_name,
            message,
            comment_type,
        });
    }

    with_dt.sort_by_key(|e| e.posted_at);
    Ok(with_dt)
}

/// `video_start` を基準に `start_sec` を埋める。動画開始より前のコメントは除外する。
/// 同一時刻の重複開始はわずかにずらし、ASS セグメントがゼロ長にならないようにする。
/// `end_sec` は「次コメントの開始秒」（最終行は未使用に近いがフィールド互換のため保持）。
pub fn annotate_timeline(entries: &mut Vec<CommentEntry>, video_start: NaiveDateTime) {
    entries.retain(|e| e.posted_at >= video_start);

    for e in entries.iter_mut() {
        let ms = e
            .posted_at
            .signed_duration_since(video_start)
            .num_milliseconds();
        e.start_sec = ms as f64 / 1000.0;
    }

    let n = entries.len();
    for i in 1..n {
        if entries[i].start_sec <= entries[i - 1].start_sec {
            entries[i].start_sec = entries[i - 1].start_sec + 0.001;
        }
    }

    for i in 0..n {
        let next_start = entries
            .get(i + 1)
            .map(|e| e.start_sec)
            .unwrap_or(f64::INFINITY);
        entries[i].end_sec = next_start;
    }
}

/// 同じ `start_sec` のコメントをまとめ、ASS 用の表示セグメント `(代表インデックス, 開始秒, 終了秒)` を返す。
pub fn build_display_segments(entries: &[CommentEntry], video_duration_sec: f64) -> Vec<(usize, f64, f64)> {
    let n = entries.len();
    if n == 0 {
        return Vec::new();
    }
    let dur = video_duration_sec.max(0.0);
    let mut out: Vec<(usize, f64, f64)> = Vec::new();
    let mut k = 0usize;
    while k < n {
        let t0 = entries[k].start_sec;
        let mut last = k;
        while last + 1 < n && (entries[last + 1].start_sec - t0).abs() < 1e-6 {
            last += 1;
        }
        let t1 = if last + 1 < n {
            entries[last + 1].start_sec
        } else {
            dur
        };
        let end = t1.max(t0 + 0.02).min(dur.max(t0 + 0.02));
        if end > t0 + 1e-6 {
            out.push((last, t0, end));
        }
        k = last + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_at(sec: f64) -> CommentEntry {
        CommentEntry {
            posted_at: NaiveDateTime::MIN,
            start_sec: sec,
            end_sec: 0.0,
            display_name: String::new(),
            message: String::new(),
            comment_type: String::new(),
        }
    }

    #[test]
    fn segments_cover_until_next_and_duration() {
        let mut e = vec![
            entry_at(0.0),
            entry_at(1.0),
            entry_at(5.0),
        ];
        e[0].end_sec = 1.0;
        e[1].end_sec = 5.0;
        e[2].end_sec = 99.0;
        let segs = build_display_segments(&e, 10.0);
        assert_eq!(segs.len(), 3);
        assert!((segs[0].1 - 0.0).abs() < 1e-6 && (segs[0].2 - 1.0).abs() < 1e-3);
        assert!((segs[1].1 - 1.0).abs() < 1e-6 && (segs[1].2 - 5.0).abs() < 1e-3);
        assert!((segs[2].1 - 5.0).abs() < 1e-6 && (segs[2].2 - 10.0).abs() < 1e-3);
    }

    #[test]
    fn segments_merge_same_timestamp() {
        let mut e = vec![entry_at(1.0), entry_at(1.0), entry_at(3.0)];
        e[2].start_sec = 3.0;
        let segs = build_display_segments(&e, 10.0);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].0, 1);
        assert!((segs[0].2 - 3.0).abs() < 1e-3);
    }
}
