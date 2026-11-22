//! 時間の帯の並べ方（純な計算。DB に触らない）: 行（アクター）ごとの居た間・model と effort の替わり目・出来事の印と、
//! 線と、時刻の窓から、帯と矢印と印と目盛りの置き場を出す。
//!
//! - 時刻は UNIX の時からのミリ秒（castlist が受けた時刻）。横が時刻で、左から右へ進む。
//! - 行は 1 つのアクターに 1 本。人を先に、ほかは並べる順の鍵（初めの登場の時刻）の順に上から。
//! - 帯は、居る出来事（[`Presence::In`]）から居ない出来事（[`Presence::Out`]）まで。退いた後にまた来れば、帯は別になる。
//!   居るままなら、窓の右の端（窓が無ければ今）まで描き、端を開いた形にする。
//! - 帯は、model と effort の替わり目（与えた順 = seq の順）で区切る。同じ値が続く物は区切らない。
//! - 帯の字は、帯の幅に収まる時だけ書く（はみ出さない）。
//! - 同じ入力からは、いつも同じ答え。

/// 図の縁の余白。
pub const MARGIN: u32 = 16;
/// 上の目盛りの字の段の高さ。
pub const AXIS_H: u32 = 28;
/// 行の高さと、その中の帯の高さ。
pub const ROW_H: u32 = 30;
pub const BAND_H: u32 = 18;
/// 名の列と時刻の間の隙間。
pub const GAP: u32 = 12;
/// 時刻の部分の幅。
pub const PLOT_W: u32 = 960;
/// 開いた端の印の分、右に空ける幅。
pub const END_ROOM: u32 = 10;
/// 帯の字の大きさ（px。site.css の `.timeline .seg-label` と同じ）と、帯の中の左右の余白。
pub const LABEL_PX: u32 = 11;
pub const LABEL_PAD: u32 = 4;
/// 時刻の窓の短さの下限（ミリ秒）。データがこれより短ければ、右に延ばす。
pub const MIN_SPAN_MS: i64 = 60_000;
/// 目盛りの間の狭さの下限（px。字が重ならないように）。
pub const MIN_TICK_PX: u32 = 100;

/// 居る・居ないを替える出来事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// 登場・再登場・分身。
    In,
    /// 交代・退場・幕引き。
    Out,
}

/// model と effort の替わり目: 受けた時刻と、帯に書く字（同じ字が続けば区切らない）。
#[derive(Clone, Debug)]
pub struct Change {
    pub at: i64,
    pub label: String,
}

/// 行 1 つ（アクター 1 つ）。どの並びも seq の順。
#[derive(Clone, Debug, Default)]
pub struct Lane {
    pub human: bool,
    /// 並べる順の鍵（初めの登場の時刻。小さい方が上）。
    pub order: i64,
    /// 名の列に要る幅。
    pub label_width: u32,
    pub presence: Vec<(i64, Presence)>,
    pub changes: Vec<Change>,
    /// 出来事の印の時刻。
    pub marks: Vec<i64>,
}

/// 線の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 起こした（実線）。起こした者の行の、起こした時刻から、起こされた者の帯の初めへ。
    Launched,
    /// 継いだ（点線）。前の帯の終わりから、後の帯の初めへ。
    Continued,
}

/// 線: 行の番号（`lanes` の添字）で、元から先へ。`at` は線を受けた時刻。
#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    pub kind: Kind,
    pub at: i64,
}

/// 時刻の窓。`from`・`to` が無ければ、データの初めから終わり（居る者がいれば今）まで。
/// `offset_s` は目盛りの字の時刻のずれ（秒。日本の時刻なら 9 時間）。
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub now: i64,
    pub offset_s: i64,
}

/// 帯の切れ 1 つ。`change` は、その間の model と effort（`lanes[lane].changes` の添字。まだ無ければ None）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub lane: usize,
    pub change: Option<usize>,
    /// 窓で切った後の時刻。
    pub start: i64,
    pub end: i64,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// 窓の左の端で切った。
    pub open_start: bool,
    /// 居るまま（か、窓の右の端で切った）。
    pub open_end: bool,
    /// 帯の字を書けるか（幅に収まるか）。
    pub labelled: bool,
}

/// 矢印。`links` の添字と、元の点・先の点と、SVG の path の `d`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrow {
    pub link: usize,
    pub kind: Kind,
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    pub d: String,
}

/// 出来事の印（縦の短い線）。`mark` は `lanes[lane].marks` の添字。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkAt {
    pub lane: usize,
    pub mark: usize,
    pub x: u32,
    pub y0: u32,
    pub y1: u32,
}

/// 目盛り: 横の置き場と字。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tick {
    pub x: u32,
    pub label: String,
}

/// 並べた答え。`rows` は `lanes` と同じ添字（行の上の y）。`order` は上からの行の順（`lanes` の添字）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// 時刻の窓（ミリ秒）。
    pub start: i64,
    pub end: i64,
    /// 時刻の部分の左の端と幅。
    pub plot_x: u32,
    pub plot_w: u32,
    pub order: Vec<usize>,
    pub rows: Vec<u32>,
    pub segments: Vec<Segment>,
    pub arrows: Vec<Arrow>,
    pub marks: Vec<MarkAt>,
    pub ticks: Vec<Tick>,
}

impl Layout {
    /// 時刻の横の置き場（窓の外は端に寄せる）。
    pub fn x(&self, _t: i64) -> u32 {
        0
    }
}

/// 行と線を並べる。
pub fn layout(_lanes: &[Lane], _links: &[Link], _window: Window) -> Layout {
    Layout::default()
}

/// 目盛り: 窓の長さに合わせて、秒・分・時・日の切りのよい間（`offset_s` だけずらした時刻で）を選ぶ。
/// 間は `MIN_TICK_PX` より狭くしない。字は、間が 1 日より短ければ `時:分`（秒の間なら `時:分:秒`）で、
/// 初めの目盛りと日が替わった目盛りには `月-日` を前に付ける。1 日より長ければ `月-日`。
pub fn ticks(_start: i64, _end: i64, _plot_x: u32, _plot_w: u32, _offset_s: i64) -> Vec<Tick> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{
        BAND_H, Change, Kind, LABEL_PAD, LABEL_PX, Lane, Layout, Link, MIN_TICK_PX, PLOT_W,
        Presence, ROW_H, Window, layout, ticks,
    };
    use crate::tree::text_width;

    /// 2026-11-22T04:00:00Z（日本の時刻で 13:00）。
    const T0: i64 = 1_795_320_000_000;
    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    const JST: i64 = 9 * 3600;

    fn window(from: Option<i64>, to: Option<i64>, now: i64) -> Window {
        Window {
            from,
            to,
            now,
            offset_s: JST,
        }
    }

    /// T0 から T0 + 60 分の窓。
    fn hour() -> Window {
        window(Some(T0), Some(T0 + HOUR), T0 + 2 * HOUR)
    }

    fn lane(order: i64, presence: &[(i64, Presence)]) -> Lane {
        Lane {
            human: false,
            order,
            label_width: 80,
            presence: presence.to_vec(),
            changes: Vec::new(),
            marks: Vec::new(),
        }
    }

    fn change(at: i64, label: &str) -> Change {
        Change {
            at,
            label: label.to_owned(),
        }
    }

    /// 窓の中の時刻の、横の置き場（並べ方と同じ式: 左の端 + 窓の初めからの長さ × 幅 ÷ 窓の長さ、切り捨て）。
    fn expect_x(l: &Layout, t: i64) -> u32 {
        let off = (t - l.start) * i64::from(l.plot_w) / (l.end - l.start);
        l.plot_x + u32::try_from(off).unwrap()
    }

    fn segments_of(l: &Layout, lane: usize) -> Vec<&super::Segment> {
        let mut s: Vec<_> = l.segments.iter().filter(|s| s.lane == lane).collect();
        s.sort_by_key(|s| s.start);
        s
    }

    #[test]
    fn 帯の端は_登場と退場の時刻の横の置き場() {
        let lanes = vec![lane(
            T0,
            &[
                (T0 + 10 * MIN, Presence::In),
                (T0 + 40 * MIN, Presence::Out),
            ],
        )];
        let l = layout(&lanes, &[], hour());
        assert_eq!((l.start, l.end), (T0, T0 + HOUR));
        assert_eq!(l.plot_w, PLOT_W);
        let s = segments_of(&l, 0);
        assert_eq!(s.len(), 1, "{l:?}");
        assert_eq!(s[0].x, expect_x(&l, T0 + 10 * MIN));
        assert_eq!(s[0].x + s[0].w, expect_x(&l, T0 + 40 * MIN));
        assert_eq!(l.x(T0 + 10 * MIN), s[0].x);
        // 10 分は 960 px の 1/6。
        assert_eq!(s[0].x, l.plot_x + 160);
        assert_eq!(s[0].w, 480);
        assert_eq!(s[0].h, BAND_H);
        assert!(!s[0].open_start && !s[0].open_end);
        // 帯は行の中の真ん中。
        assert_eq!(s[0].y, l.rows[0] + (ROW_H - BAND_H) / 2);
    }

    #[test]
    fn 帯は_model_と_effort_の替わり目で_与えた順に区切る() {
        let mut a = lane(
            T0,
            &[
                (T0 + 10 * MIN, Presence::In),
                (T0 + 50 * MIN, Presence::Out),
            ],
        );
        a.changes = vec![
            // 登場の前に頼んだ物は、帯の初めから効く。
            change(T0, "opus-5-5 · high"),
            change(T0 + 20 * MIN, "sonnet-5 · high"),
            // 同じ値が続けば区切らない。
            change(T0 + 25 * MIN, "sonnet-5 · high"),
            change(T0 + 30 * MIN, "opus-5-5 · high"),
            // 同じ時刻に 2 つ来れば、後（seq の大きい方）が効く。
            change(T0 + 40 * MIN, "opus-5-5 · max"),
            change(T0 + 40 * MIN, "opus-5-5 · low"),
        ];
        let l = layout(&[a], &[], hour());
        let s = segments_of(&l, 0);
        let got: Vec<(i64, i64, Option<usize>)> = s
            .iter()
            .map(|s| ((s.start - T0) / MIN, (s.end - T0) / MIN, s.change))
            .collect();
        assert_eq!(
            got,
            vec![
                (10, 20, Some(0)),
                (20, 30, Some(1)),
                (30, 40, Some(3)),
                (40, 50, Some(5))
            ],
            "{l:?}"
        );
        // 切れは隙間なく続く。
        for pair in s.windows(2) {
            assert_eq!(pair[0].x + pair[0].w, pair[1].x, "{pair:?}");
        }
    }

    #[test]
    fn 替わり目の前の間は_model_と_effort_が無い() {
        let mut a = lane(
            T0,
            &[
                (T0 + 10 * MIN, Presence::In),
                (T0 + 30 * MIN, Presence::Out),
            ],
        );
        a.changes = vec![change(T0 + 20 * MIN, "opus-5-5 · high")];
        let l = layout(&[a], &[], hour());
        let s = segments_of(&l, 0);
        assert_eq!(s.len(), 2, "{l:?}");
        assert_eq!(s[0].change, None);
        assert_eq!(s[1].change, Some(0));
    }

    #[test]
    fn 退いた後にまた来れば_帯は_2_つ() {
        let lanes = vec![lane(
            T0,
            &[
                (T0 + 5 * MIN, Presence::In),
                (T0 + 15 * MIN, Presence::Out),
                (T0 + 30 * MIN, Presence::In),
                // 居る間の 2 度目の登場（分身など）は、帯を割らない。
                (T0 + 35 * MIN, Presence::In),
                (T0 + 45 * MIN, Presence::Out),
                // 居ない間の 2 度目の退場は、何もしない。
                (T0 + 50 * MIN, Presence::Out),
            ],
        )];
        let l = layout(&lanes, &[], hour());
        let s = segments_of(&l, 0);
        let got: Vec<(i64, i64)> = s
            .iter()
            .map(|s| ((s.start - T0) / MIN, (s.end - T0) / MIN))
            .collect();
        assert_eq!(got, vec![(5, 15), (30, 45)], "{l:?}");
        assert!(s[0].x + s[0].w < s[1].x);
    }

    #[test]
    fn 居る者の帯は_今か窓の右の端まで描き_端を開く() {
        let lanes = vec![
            lane(T0, &[(T0 + 10 * MIN, Presence::In)]),
            lane(
                T0 + MIN,
                &[
                    (T0 + 11 * MIN, Presence::In),
                    (T0 + 20 * MIN, Presence::Out),
                ],
            ),
        ];
        // 窓が無ければ、今まで。
        let now = T0 + 50 * MIN;
        let l = layout(&lanes, &[], window(None, None, now));
        assert_eq!((l.start, l.end), (T0 + 10 * MIN, now), "{l:?}");
        let open = segments_of(&l, 0);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].end, now);
        assert_eq!(open[0].x + open[0].w, expect_x(&l, now));
        assert!(open[0].open_end);
        let closed = segments_of(&l, 1);
        assert!(!closed[0].open_end);
        // 窓の右の端で切れば、そこまで。退場が窓の外でも、端を開く。
        let l = layout(&lanes, &[], window(Some(T0), Some(T0 + 15 * MIN), now));
        for lane in [0, 1] {
            let s = segments_of(&l, lane);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].end, T0 + 15 * MIN);
            assert_eq!(s[0].x + s[0].w, l.plot_x + l.plot_w);
            assert!(s[0].open_end, "{lane}: {s:?}");
        }
        // 窓の左の端で切れば、左を開く。窓の外の帯は描かない。
        let l = layout(
            &lanes,
            &[],
            window(Some(T0 + 15 * MIN), Some(T0 + 60 * MIN), now),
        );
        let s = segments_of(&l, 1);
        assert_eq!(s.len(), 1);
        assert!(s[0].open_start && !s[0].open_end, "{s:?}");
        let l = layout(
            &lanes,
            &[],
            window(Some(T0 + 30 * MIN), Some(T0 + 60 * MIN), now),
        );
        assert!(segments_of(&l, 1).is_empty(), "{l:?}");
    }

    #[test]
    fn 窓が無ければ_データの初めから終わりまで_短ければ_1_分に延ばす() {
        let lanes = vec![lane(
            T0,
            &[(T0 + 1000, Presence::In), (T0 + 3000, Presence::Out)],
        )];
        let l = layout(&lanes, &[], window(None, None, T0 + 10 * HOUR));
        assert_eq!(l.start, T0 + 1000);
        assert_eq!(l.end, T0 + 1000 + 60_000);
    }

    #[test]
    fn 人を上に_ほかは初めの登場の順に並べる() {
        let mut lanes = vec![
            lane(T0 + 3 * MIN, &[(T0 + 3 * MIN, Presence::In)]),
            lane(T0 + MIN, &[(T0 + MIN, Presence::In)]),
            lane(T0 + 5 * MIN, &[(T0 + 5 * MIN, Presence::In)]),
            lane(T0 + 2 * MIN, &[(T0 + 2 * MIN, Presence::In)]),
        ];
        lanes[2].human = true;
        let l = layout(&lanes, &[], hour());
        assert_eq!(l.order, vec![2, 1, 3, 0]);
        let ys: Vec<u32> = l.order.iter().map(|&i| l.rows[i]).collect();
        for pair in ys.windows(2) {
            assert_eq!(pair[1], pair[0] + ROW_H, "{ys:?}");
        }
        assert!(l.height >= ys[3] + ROW_H);
    }

    #[test]
    fn 起こした矢印は_起こした者の行の起こした時刻から_起こされた者の帯の初めへ() {
        let lanes = vec![
            lane(T0, &[(T0 + 5 * MIN, Presence::In)]),
            lane(T0 + 20 * MIN, &[(T0 + 20 * MIN, Presence::In)]),
        ];
        let links = [Link {
            from: 0,
            to: 1,
            kind: Kind::Launched,
            at: T0 + 15 * MIN,
        }];
        let l = layout(&lanes, &links, hour());
        assert_eq!(l.arrows.len(), 1, "{l:?}");
        let a = &l.arrows[0];
        assert_eq!(a.kind, Kind::Launched);
        assert_eq!(a.x0, expect_x(&l, T0 + 15 * MIN));
        assert_eq!(a.x1, expect_x(&l, T0 + 20 * MIN));
        // 元は起こした者の帯の下の縁、先は起こされた者の帯の真ん中の高さ。
        assert_eq!(a.y0, l.rows[0] + (ROW_H + BAND_H) / 2);
        assert_eq!(a.y1, l.rows[1] + ROW_H / 2);
        assert!(a.d.starts_with(&format!("M{},{}", a.x0, a.y0)), "{a:?}");
        assert!(a.d.ends_with(&format!("H{}", a.x1)), "{a:?}");
    }

    #[test]
    fn 継いだ矢印は_前の帯の終わりから_後の帯の初めへ() {
        let lanes = vec![
            lane(
                T0,
                &[(T0 + 5 * MIN, Presence::In), (T0 + 20 * MIN, Presence::Out)],
            ),
            lane(T0 + 30 * MIN, &[(T0 + 30 * MIN, Presence::In)]),
        ];
        let links = [Link {
            from: 0,
            to: 1,
            kind: Kind::Continued,
            at: T0 + 21 * MIN,
        }];
        let l = layout(&lanes, &links, hour());
        assert_eq!(l.arrows.len(), 1, "{l:?}");
        let a = &l.arrows[0];
        assert_eq!(a.kind, Kind::Continued);
        assert_eq!(a.x0, expect_x(&l, T0 + 20 * MIN));
        assert_eq!(a.x1, expect_x(&l, T0 + 30 * MIN));
    }

    #[test]
    fn 上の行へ向かう矢印は_帯の上の縁から出る() {
        // 後の行の者が、前の行の者を起こした（時計の都合など）。
        let lanes = vec![
            lane(T0 + 20 * MIN, &[(T0 + 20 * MIN, Presence::In)]),
            lane(T0, &[(T0 + 5 * MIN, Presence::In)]),
        ];
        let links = [Link {
            from: 0,
            to: 1,
            kind: Kind::Launched,
            at: T0 + 25 * MIN,
        }];
        let l = layout(&lanes, &links, hour());
        let a = &l.arrows[0];
        assert!(l.rows[0] > l.rows[1]);
        assert_eq!(a.y0, l.rows[0] + (ROW_H - BAND_H) / 2);
    }

    #[test]
    fn 出来事の印は_その時刻の横の置き場で_窓の外は描かない() {
        let mut a = lane(T0, &[(T0, Presence::In)]);
        a.marks = vec![T0 + 30 * MIN, T0 + 2 * HOUR];
        let l = layout(&[a], &[], hour());
        assert_eq!(l.marks.len(), 1, "{l:?}");
        assert_eq!(l.marks[0].mark, 0);
        assert_eq!(l.marks[0].x, expect_x(&l, T0 + 30 * MIN));
        assert!(l.marks[0].y0 < l.marks[0].y1);
    }

    #[test]
    fn 帯の字は_帯の幅に収まる時だけ書く() {
        let mut a = lane(T0, &[(T0, Presence::In), (T0 + 40 * MIN, Presence::Out)]);
        a.changes = vec![
            change(T0, "opus-5-5 · high"),
            // 1 分は 16 px。字は収まらない。
            change(T0 + 30 * MIN, "sonnet-5 · high"),
            change(T0 + 31 * MIN, "opus-5-5 · high"),
        ];
        let l = layout(&[a.clone()], &[], hour());
        let s = segments_of(&l, 0);
        assert_eq!(s.len(), 3);
        let labelled: Vec<bool> = s.iter().map(|s| s.labelled).collect();
        assert_eq!(labelled, vec![true, false, true], "{s:?}");
        for seg in &s {
            let label = &a.changes[seg.change.unwrap()].label;
            let need = text_width(label, LABEL_PX) + LABEL_PAD * 2;
            assert_eq!(seg.labelled, need <= seg.w, "{seg:?} {need}");
        }
    }

    /// 目盛りを見る: 数は 2 から 10、間は等しく MIN_TICK_PX 以上、時刻の部分の中。
    fn check_ticks(t: &[super::Tick], plot_x: u32) {
        assert!((2..=10).contains(&t.len()), "{t:?}");
        let gap = t[1].x - t[0].x;
        assert!(gap >= MIN_TICK_PX, "{t:?}");
        for pair in t.windows(2) {
            let d = pair[1].x - pair[0].x;
            assert!(d.abs_diff(gap) <= 1, "{t:?}");
        }
        assert!(
            t.iter().all(|k| k.x >= plot_x && k.x <= plot_x + PLOT_W),
            "{t:?}"
        );
    }

    #[test]
    fn 目盛りは_分の窓なら_切りのよい分() {
        // 13:00:30 から 13:10:30（日本の時刻）。2 分ごと。
        let t = ticks(T0 + 30_000, T0 + 10 * MIN + 30_000, 100, PLOT_W, JST);
        check_ticks(&t, 100);
        let labels: Vec<&str> = t.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["11-22 13:02", "13:04", "13:06", "13:08", "13:10"]
        );
    }

    #[test]
    fn 目盛りは_時間の窓なら_切りのよい時() {
        // 13:00 から 19:00。1 時間ごと。
        let t = ticks(T0, T0 + 6 * HOUR, 100, PLOT_W, JST);
        check_ticks(&t, 100);
        let labels: Vec<&str> = t.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "11-22 13:00",
                "14:00",
                "15:00",
                "16:00",
                "17:00",
                "18:00",
                "19:00"
            ]
        );
        assert_eq!(t[0].x, 100);
        // 日を越えれば、日を付ける。
        let t = ticks(T0 + 9 * HOUR, T0 + 15 * HOUR, 100, PLOT_W, JST);
        let labels: Vec<&str> = t.iter().map(|k| k.label.as_str()).collect();
        assert!(labels.contains(&"11-23 00:00"), "{labels:?}");
    }

    #[test]
    fn 目盛りは_日の窓なら_日本の時刻の_0_時() {
        // 11-22 13:00 から 11-27 13:00。1 日ごと。初めは 11-23 の 0 時（UTC の 11-22 15:00）。
        let t = ticks(T0, T0 + 5 * DAY, 100, PLOT_W, JST);
        check_ticks(&t, 100);
        let labels: Vec<&str> = t.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(labels, vec!["11-23", "11-24", "11-25", "11-26", "11-27"]);
        let first = (11 * HOUR) * i64::from(PLOT_W) / (5 * DAY);
        assert_eq!(t[0].x, 100 + u32::try_from(first).unwrap());
    }

    #[test]
    fn 目盛りの字は重ならない() {
        for span in [
            MIN,
            7 * MIN,
            50 * MIN,
            3 * HOUR,
            20 * HOUR,
            3 * DAY,
            40 * DAY,
        ] {
            let t = ticks(T0 + 7_777, T0 + 7_777 + span, 0, PLOT_W, JST);
            assert!(!t.is_empty(), "{span}");
            for pair in t.windows(2) {
                let w = text_width(&pair[0].label, LABEL_PX);
                // 字は目盛りの真ん中に置く。
                assert!(
                    pair[0].x + w / 2 + 8 <= pair[1].x - text_width(&pair[1].label, LABEL_PX) / 2,
                    "{span}: {pair:?}"
                );
            }
        }
    }

    #[test]
    fn 同じ入力からは同じ答え() {
        let mut a = lane(T0, &[(T0 + 5 * MIN, Presence::In)]);
        a.changes = vec![change(T0, "opus-5-5 · high"), change(T0 + 9 * MIN, "x")];
        a.marks = vec![T0 + 7 * MIN];
        let lanes = vec![a, lane(T0 + 8 * MIN, &[(T0 + 8 * MIN, Presence::In)])];
        let links = [Link {
            from: 0,
            to: 1,
            kind: Kind::Launched,
            at: T0 + 8 * MIN,
        }];
        let x = layout(&lanes, &links, window(None, None, T0 + 30 * MIN));
        let y = layout(&lanes, &links, window(None, None, T0 + 30 * MIN));
        assert!(!x.segments.is_empty() && !x.ticks.is_empty());
        assert_eq!(x, y);
    }

    #[test]
    fn 行が無ければ_空() {
        let l = layout(&[], &[], hour());
        assert!(l.segments.is_empty() && l.rows.is_empty());
    }
}
