//! 左パネル用 ASS 字幕の生成（本文は白字・黒縁、表示名は見やすい緑・黒縁、クリップ、最新を上に積み下へ伸びる表示）。
//!
//! 改行: Unicode Line Breaking Algorithm（unicode-linebreak）で区切り候補を選び、
//! unicode-width で表示幅を積算して物理行に分割。libass は **\\q2**（自動改行オフ）とし、
//! **\\N による強制改行のみ**を解釈させる（誤って **\\q3** を付けると wrap_style 3 となり
//! 自動折り返しが再び有効になり、手動改行と競合する）。

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use unicode_linebreak::{linebreaks, BreakOpportunity};
use unicode_width::UnicodeWidthChar;

use crate::comments::CommentEntry;

/// ASS レイアウトパラメータ。
pub struct AssLayout<'a> {
    pub play_res_x: u32,
    pub play_res_y: u32,
    pub panel_width: u32,
    pub font_name: &'a str,
    /// 本文のフォントサイズ
    pub font_size: i32,
    /// 表示名（リスナー名）のフォントサイズ（本文より小さくする）
    pub name_font_size: i32,
    pub max_lines: u32,
    /// パネルに残す過去コメントの最大経過秒（これより古い行はスタックから除外）
    pub stack_retention_sec: f64,
    /// 新コメントが上に付くときの落下アニメの時間（ミリ秒）
    pub scroll_ms: i64,
}

/// ASS 用に `{` `}` `\` をエスケープし、改行を `\N` にする。
pub fn escape_ass_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' | '\r' => out.push_str("\\N"),
            _ => out.push(ch),
        }
    }
    out
}

#[inline]
fn display_width_range(s: &str, start: usize, end: usize) -> usize {
    s.get(start..end)
        .map(|sub| {
            sub.chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
                .sum()
        })
        .unwrap_or(0)
}

/// Unicode 行区切り候補（Mandatory / Allowed）と表示幅 `limit` に基づき物理行へ分割する。
fn wrap_by_display_width(s: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(4);
    if s.is_empty() {
        return vec![String::new()];
    }

    let break_starts: Vec<usize> = linebreaks(s)
        .filter(|(_, op)| matches!(op, BreakOpportunity::Mandatory | BreakOpportunity::Allowed))
        .map(|(idx, _)| idx)
        .collect();

    let mut out: Vec<String> = Vec::new();
    let mut line_start = 0usize;
    let mut line_width = 0usize;

    for (byte_i, ch) in s.char_indices() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(1);
        let ch_end = byte_i + ch.len_utf8();

        if line_width + w > limit {
            if byte_i > line_start {
                let cut = break_starts
                    .iter()
                    .rev()
                    .find(|&&b| b > line_start && b <= byte_i)
                    .copied()
                    .unwrap_or(byte_i);
                out.push(s[line_start..cut].to_string());
                line_start = cut;
                line_width = display_width_range(s, line_start, byte_i);
            } else {
                out.push(s[byte_i..ch_end].to_string());
                line_start = ch_end;
                line_width = 0;
                continue;
            }
        }

        line_width += w;
    }

    if line_start < s.len() {
        out.push(s[line_start..].to_string());
    }

    let out: Vec<String> = out.into_iter().filter(|l| !l.is_empty()).collect();
    if out.is_empty() && !s.is_empty() {
        vec![s.to_string()]
    } else {
        out
    }
}

/// パネル内の利用可能幅（px）とフォントサイズから、1 行あたりの表示幅上限（unicode 列の合計）を求める。
///
/// 全角は [UnicodeWidthChar] で幅 2 と数え、**1 全角 ≒ `font_px` px** のとき列上限は **`2 * (px / fp)`**。
/// 以前の `0.90 * … floor` は常に右側に余白が残るため廃止。実際のフォントはアドバンスが em よりやや狭いことが多いので
/// **わずかに上振れ（`COLS_FILL_RATIO`）し `ceil`** して左右余白に近づける（大きすぎると右端がわずかにクリップ）。
const COLS_FILL_RATIO: f64 = 1.30;

fn cols_budget(text_width_px: u32, font_px: i32) -> usize {
    let px = text_width_px.max(24) as f64;
    let fp = font_px.max(6) as f64;
    let cols = (2.0 * px / fp * COLS_FILL_RATIO)
        .ceil()
        .max(8.0)
        .min(320.0) as usize;
    cols
}

/// パネル左端から本文までの余白（`MarginL` / `\\pos` の x）。
const PANEL_PAD_LEFT_PX: i32 = 10;
/// 映像直前に残す余白。左より小さくすると 1 行が長くなり右の空きが減る（左右非対称）。
const PANEL_PAD_RIGHT_PX: i32 = 1;

/// 秒を ASS 時刻 `H:MM:SS.cc`（センチ秒）に変換する。
fn format_ass_time(sec: f64) -> String {
    let cs_total = (sec * 100.0).round().max(0.0) as i64;
    let cs = cs_total % 100;
    let s_total = cs_total / 100;
    let s = s_total % 60;
    let m_total = s_total / 60;
    let m = m_total % 60;
    let h = m_total / 60;
    format!("{}:{:02}:{:02}.{:02}", h, m, s, cs)
}

/// ASS 表示名の塗り色（`\c`、不透明 &H00BBGGRR。黒背景向けの明るめグリーン）。
const DISPLAY_NAME_FILL_ASS: &str = "00B4FF7F";
/// 本文は白へ明示的に戻す（表示名の色が残らないようにする）。
const BODY_FILL_ASS: &str = "00FFFFFF";

/// 1 コメントブロック（表示名・本文それぞれ折り返し、間は `\\N`）。表示名は緑、本文は白。
fn format_comment_block(
    e: &CommentEntry,
    name_fs: i32,
    body_fs: i32,
    name_cols: usize,
    body_cols: usize,
) -> String {
    let name_lines = wrap_by_display_width(&e.display_name, name_cols);
    let body_lines = wrap_by_display_width(&e.message, body_cols);
    let name_joined = name_lines
        .iter()
        .map(|s| escape_ass_text(s.as_str()))
        .collect::<Vec<_>>()
        .join("\\N");
    let body_joined = body_lines
        .iter()
        .map(|s| escape_ass_text(s.as_str()))
        .collect::<Vec<_>>()
        .join("\\N");
    format!(
        "{{\\fs{}\\c&H{}&}}{}\\N{{\\fs{}\\c&H{}&}}{}",
        name_fs, DISPLAY_NAME_FILL_ASS, name_joined, body_fs, BODY_FILL_ASS, body_joined
    )
}

/// 代表インデックス `rep_idx` の時点のスタック本文。**最新が先頭**（上）、古いコメントが `\N` で下に続く。
fn stacked_body(
    entries: &[CommentEntry],
    rep_idx: usize,
    max_lines: u32,
    stack_retention_sec: f64,
    name_fs: i32,
    body_fs: i32,
    text_width_px: u32,
) -> String {
    let name_cols = cols_budget(text_width_px, name_fs);
    let body_cols = cols_budget(text_width_px, body_fs);
    let max = max_lines.max(1) as usize;
    let t = entries[rep_idx].start_sec;
    let mut js: Vec<usize> = (0..=rep_idx)
        .filter(|&j| t - entries[j].start_sec <= stack_retention_sec + 1e-9)
        .collect();
    if js.len() > max {
        js = js[js.len() - max..].to_vec();
    }
    let mut lines: Vec<String> = Vec::new();
    for &j in js.iter().rev() {
        lines.push(format_comment_block(
            &entries[j],
            name_fs,
            body_fs,
            name_cols,
            body_cols,
        ));
    }
    lines.join("\\N")
}

/// コメントタイムラインから ASS ファイルを書き出す。
/// `segments` は `build_display_segments` の結果（ギャップなく次コメントまで連続）。
pub fn write_ass_file(
    path: &Path,
    entries: &[CommentEntry],
    segments: &[(usize, f64, f64)],
    layout: AssLayout<'_>,
) -> Result<()> {
    let mut f = File::create(path).with_context(|| format!("create {:?}", path))?;

    // 左は従来どおり広め、右だけ映像寄りまで詰めて折り返し幅を広げる（非対称）。
    let margin_l = PANEL_PAD_LEFT_PX;
    // ASS: 有効幅 = PlayResX - MarginL - MarginR = W_panel - 左パディング - 右パディング
    let margin_r = i32::try_from(layout.play_res_x)
        .unwrap_or(i32::MAX)
        .saturating_sub(layout.panel_width as i32)
        .saturating_add(PANEL_PAD_RIGHT_PX);
    let clip_w = layout.panel_width as i32;

    let text_width_px = i32::try_from(layout.play_res_x)
        .unwrap_or(i32::MAX)
        .saturating_sub(margin_l)
        .saturating_sub(margin_r)
        .max(24) as u32;

    writeln!(f, "[Script Info]")?;
    writeln!(f, "Title: make-comment-movie")?;
    writeln!(f, "ScriptType: v4.00+")?;
    // 2: 自動改行なし、\\N と（wrap_style 2 では）\\n は強制改行（libass 公式）
    writeln!(f, "WrapStyle: 2")?;
    writeln!(f, "PlayResX: {}", layout.play_res_x)?;
    writeln!(f, "PlayResY: {}", layout.play_res_y)?;
    writeln!(f)?;
    writeln!(f, "[V4+ Styles]")?;
    writeln!(
        f,
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding"
    )?;
    writeln!(
        f,
        "Style: LeftPanel,{},{},&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,1,0,7,{},{},24,1",
        layout.font_name,
        layout.font_size,
        margin_l,
        margin_r
    )?;
    writeln!(f)?;
    writeln!(f, "[Events]")?;
    writeln!(
        f,
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
    )?;

    let pos_x = margin_l;
    let pos_y = 20_i32;
    let line_h = layout.name_font_size + layout.font_size + 14;

    for &(rep_idx, seg_start, seg_end) in segments {
        let start = format_ass_time(seg_start);
        let end = format_ass_time(seg_end);
        let body = stacked_body(
            entries,
            rep_idx,
            layout.max_lines,
            layout.stack_retention_sec,
            layout.name_font_size,
            layout.font_size,
            text_width_px,
        );
        let dur_ms = ((seg_end - seg_start) * 1000.0).max(50.0);
        let scroll_ms = layout.scroll_ms.min((dur_ms * 0.85) as i64).max(80);
        let y_top = pos_y;
        let y_from = y_top - (line_h / 2).max(10);

        // \\q2: 自動折り返し無効。\\N のみで行分割（\\q3 は wrap_style 3 で自動折り返しが復活する）
        let overrides = if scroll_ms >= 90 && dur_ms >= 120.0 {
            format!(
                "{{\\clip(0,0,{},{})\\an7\\q2\\move({},{},{},{},0,{})}}",
                clip_w,
                layout.play_res_y,
                pos_x,
                y_from,
                pos_x,
                y_top,
                scroll_ms
            )
        } else {
            format!(
                "{{\\clip(0,0,{},{})\\an7\\q2\\pos({},{})}}",
                clip_w,
                layout.play_res_y,
                pos_x,
                y_top
            )
        };

        let text = format!("{}{}", overrides, body);
        writeln!(
            f,
            "Dialogue: 0,{},{},LeftPanel,,{},{},24,,{}",
            start, end, margin_l, margin_r, text
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_cjk_run() {
        let s = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ";
        let lines = wrap_by_display_width(s, 10);
        assert!(
            lines.len() >= 3,
            "expected multiple lines, got {:?}",
            lines
        );
        for ln in &lines {
            let w: usize = ln
                .chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
                .sum();
            assert!(w <= 10, "line {:?} width {} > 10", ln, w);
        }
    }

    #[test]
    fn wraps_at_space_en() {
        let s = "hello world this is a long line of text";
        let lines = wrap_by_display_width(s, 12);
        assert!(lines.len() >= 2);
        for ln in &lines {
            let w: usize = ln
                .chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
                .sum();
            assert!(w <= 12, "line {:?} width {}", ln, w);
        }
    }

    #[test]
    fn cols_budget_reasonable() {
        let c = cols_budget(150, 22);
        // 2 * 150/22 * 1.30 ≈ 17.73 → ceil 18
        assert!((17..=32).contains(&c), "cols_budget={}", c);
    }
}
