//! 系図の並べ方（純な計算。DB に触らない）: 点（箱の幅と並べる順の鍵）と線から、箱の置き場と線の道を出す。
//!
//! - 上から下への木。起こした（[`Kind::Launched`]）線の元を親として上に、先を子として下に置く。
//! - 根は、組の中に起こした者がいないアクター（人の根など）。
//! - 兄弟は、並べる順の鍵（初めの登場の時刻）の順に左から。
//! - 継いだ（[`Kind::Continued`]）先は、起こした線が無ければ、元のすぐ右の兄弟に置く（元に親が無ければ、元の隣の根）。
//! - 部分木ごとに横の幅を取って並べるので、箱は重ならない。同じ入力からは、いつも同じ答え。

/// 箱の高さ（3 行）。
pub const BOX_H: u32 = 0;

/// 線の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 起こした（実線。親から子へ）。
    Launched,
    /// 継いだ（点線。前から後へ。/clear の交代・fork の分身）。
    Continued,
}

/// 点: 箱の幅と、兄弟を並べる順の鍵（初めの登場の時刻の字。小さい方が左）。
#[derive(Clone, Debug)]
pub struct Node {
    pub width: u32,
    pub order: String,
}

/// 線: 点の番号（`nodes` の添字）で、元から先へ。
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub kind: Kind,
}

/// 箱（左上の角と、幅と高さ）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 描く線: SVG の path の `d`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub from: usize,
    pub to: usize,
    pub kind: Kind,
    pub d: String,
}

/// 並べた答え。`boxes` は `nodes` と同じ添字。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub boxes: Vec<Rect>,
    pub lines: Vec<Line>,
    pub width: u32,
    pub height: u32,
}

/// 字の幅（px）の見積もり。字の大きさ `px` に、ASCII は 0.62 字、ほか（漢字・仮名・記号）は 1 字を掛けて足し、切り上げる。
pub fn text_width(_s: &str, _px: u32) -> u32 {
    0
}

/// 点と線を並べる。
pub fn layout(_nodes: &[Node], _edges: &[Edge]) -> Layout {
    Layout::default()
}

#[cfg(test)]
mod tests {
    use super::{Edge, Kind, Layout, Node, Rect, layout, text_width};

    fn node(width: u32, order: &str) -> Node {
        Node {
            width,
            order: order.to_owned(),
        }
    }

    fn edge(from: usize, to: usize, kind: Kind) -> Edge {
        Edge { from, to, kind }
    }

    /// 並べて、箱の数が点の数と同じことを先に見る。
    fn lay(nodes: &[Node], edges: &[Edge]) -> Layout {
        let l = layout(nodes, edges);
        assert_eq!(l.boxes.len(), nodes.len(), "箱の数");
        l
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    fn assert_no_overlap(l: &Layout) {
        for (i, a) in l.boxes.iter().enumerate() {
            assert!(a.w > 0 && a.h > 0, "{i}: {a:?}");
            assert!(
                a.x + a.w <= l.width && a.y + a.h <= l.height,
                "{i} が図の外: {a:?} {}x{}",
                l.width,
                l.height
            );
            for (j, b) in l.boxes.iter().enumerate().skip(i + 1) {
                assert!(!overlaps(a, b), "{i} と {j} が重なる: {a:?} {b:?}");
            }
        }
    }

    fn assert_parents_above(l: &Layout, edges: &[Edge]) {
        for e in edges.iter().filter(|e| e.kind == Kind::Launched) {
            let (p, c) = (&l.boxes[e.from], &l.boxes[e.to]);
            assert!(
                p.y + p.h < c.y,
                "親 {} が子 {} の上に無い: {p:?} {c:?}",
                e.from,
                e.to
            );
        }
    }

    const PM: usize = 0;
    const TPM: usize = 1;
    const SWE1: usize = 2;
    const SWE2: usize = 3;
    const DEVOPS: usize = 4;
    const AUDITOR: usize = 5;
    const SWE1_NEXT: usize = 6;

    /// PM → TPM → SWE×2 と DevOps、PM → Auditor。SWE の 1 つ目は /clear で退き、後が継ぐ。
    /// 入れる順は、並べる順（時刻）と違えておく（DevOps が先・SWE の 2 つ目が 1 つ目より先）。
    fn sample() -> (Vec<Node>, Vec<Edge>) {
        let nodes = vec![
            node(120, "2026-11-22T01:00:00.000Z"),
            node(150, "2026-11-22T01:01:00.000Z"),
            node(170, "2026-11-22T01:02:00.000Z"),
            node(140, "2026-11-22T01:03:00.000Z"),
            node(200, "2026-11-22T01:04:00.000Z"),
            node(130, "2026-11-22T01:05:00.000Z"),
            node(160, "2026-11-22T01:06:00.000Z"),
        ];
        let edges = vec![
            edge(TPM, DEVOPS, Kind::Launched),
            edge(PM, TPM, Kind::Launched),
            edge(TPM, SWE2, Kind::Launched),
            edge(TPM, SWE1, Kind::Launched),
            edge(PM, AUDITOR, Kind::Launched),
            edge(SWE1, SWE1_NEXT, Kind::Continued),
        ];
        (nodes, edges)
    }

    #[test]
    fn 見本の点と線を全部並べる() {
        let (nodes, edges) = sample();
        let l = lay(&nodes, &edges);
        assert_eq!(l.boxes.len(), 7);
        assert_eq!(l.lines.len(), 6);
        let count = |kind| l.lines.iter().filter(|x| x.kind == kind).count();
        assert_eq!(count(Kind::Launched), 5);
        assert_eq!(count(Kind::Continued), 1);
        for (b, n) in l.boxes.iter().zip(&nodes) {
            assert_eq!(b.w, n.width, "箱の幅は点の幅");
        }
        assert!(
            l.lines.iter().all(|x| x.d.starts_with('M')),
            "{:?}",
            l.lines
        );
    }

    #[test]
    fn 親は子より上で_根は一番上() {
        let (nodes, edges) = sample();
        let l = lay(&nodes, &edges);
        assert_parents_above(&l, &edges);
        let top = l.boxes.iter().map(|b| b.y).min().unwrap();
        assert_eq!(l.boxes[PM].y, top);
        assert!(
            l.boxes
                .iter()
                .enumerate()
                .all(|(i, b)| i == PM || b.y > top)
        );
        // 起こした者が同じなら、同じ段。
        assert_eq!(l.boxes[TPM].y, l.boxes[AUDITOR].y);
        assert_eq!(l.boxes[SWE1].y, l.boxes[DEVOPS].y);
    }

    #[test]
    fn 箱は重ならない() {
        let (nodes, edges) = sample();
        assert_no_overlap(&lay(&nodes, &edges));
    }

    #[test]
    fn 兄弟は初めの登場の順に左から() {
        let (nodes, edges) = sample();
        let l = lay(&nodes, &edges);
        let x = |i: usize| l.boxes[i].x;
        assert!(x(SWE1) < x(SWE2) && x(SWE2) < x(DEVOPS), "{:?}", l.boxes);
        assert!(x(TPM) < x(AUDITOR), "{:?}", l.boxes);
    }

    #[test]
    fn 継いだ後は_前のすぐ右の兄弟() {
        let (nodes, edges) = sample();
        let l = lay(&nodes, &edges);
        let (before, after) = (&l.boxes[SWE1], &l.boxes[SWE1_NEXT]);
        assert_eq!(before.y, after.y, "同じ段");
        assert!(after.x >= before.x + before.w, "{before:?} {after:?}");
        // 前と後の間に、ほかの兄弟は無い。
        for other in [SWE2, DEVOPS] {
            let o = &l.boxes[other];
            assert!(
                o.x >= after.x + after.w,
                "{other} が前と後の間にある: {o:?}"
            );
        }
    }

    #[test]
    fn 親の無い前を継いだ後は_前の隣の根() {
        let nodes = vec![
            node(100, "2026-11-22T01:00:00.000Z"),
            node(100, "2026-11-22T01:02:00.000Z"),
            node(100, "2026-11-22T01:01:00.000Z"),
        ];
        // 0 を 1 が継ぐ。2 は別の根で、時刻は 0 と 1 の間。
        let edges = vec![edge(0, 1, Kind::Continued)];
        let l = lay(&nodes, &edges);
        assert_eq!(l.boxes.len(), 3);
        assert_eq!(l.boxes[0].y, l.boxes[1].y);
        assert_eq!(l.boxes[0].y, l.boxes[2].y);
        assert!(l.boxes[1].x >= l.boxes[0].x + l.boxes[0].w);
        assert!(l.boxes[2].x >= l.boxes[1].x + l.boxes[1].w, "{:?}", l.boxes);
        assert_no_overlap(&l);
    }

    #[test]
    fn 同じ入力からは同じ答え() {
        let (nodes, edges) = sample();
        let a = lay(&nodes, &edges);
        let b = lay(&nodes, &edges);
        assert!(!a.boxes.is_empty());
        assert_eq!(a, b);
    }

    #[test]
    fn 輪になった線でも止まり_全部を重ねずに並べる() {
        let nodes: Vec<Node> = (0..4)
            .map(|i| node(100, &format!("2026-11-22T01:0{i}:00.000Z")))
            .collect();
        let edges = vec![
            edge(0, 1, Kind::Launched),
            edge(1, 0, Kind::Launched),
            edge(2, 3, Kind::Continued),
            edge(3, 2, Kind::Continued),
            edge(1, 1, Kind::Launched),
        ];
        let l = lay(&nodes, &edges);
        assert_eq!(l.boxes.len(), 4);
        assert_no_overlap(&l);
    }

    /// 決まった種の擬似乱数（線形合同）。
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % n
        }
    }

    #[test]
    fn 乱れた_500_の木も重ねずに並べる() {
        let mut rng = Lcg(23);
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for i in 0..500_usize {
            let width = 80 + u32::try_from(rng.next(220)).unwrap();
            let order = format!("2026-11-22T{:02}:{:02}:00.000Z", rng.next(24), rng.next(60));
            nodes.push(node(width, &order));
            if i > 0 && rng.next(20) > 0 {
                let from = usize::try_from(rng.next(i as u64)).unwrap();
                let kind = if rng.next(10) == 0 {
                    Kind::Continued
                } else {
                    Kind::Launched
                };
                edges.push(edge(from, i, kind));
            }
        }
        let l = lay(&nodes, &edges);
        assert_eq!(l.boxes.len(), 500);
        assert_eq!(l.lines.len(), edges.len());
        assert_no_overlap(&l);
        assert_parents_above(&l, &edges);
        for e in edges.iter().filter(|e| e.kind == Kind::Continued) {
            let (a, b) = (&l.boxes[e.from], &l.boxes[e.to]);
            assert!(a.y == b.y && b.x >= a.x + a.w, "{a:?} {b:?}");
        }
        assert_eq!(l, lay(&nodes, &edges));
    }

    #[test]
    fn 字の幅は_漢字と仮名を_ascii_より広く見積もる() {
        assert_eq!(text_width("", 12), 0);
        let ascii = text_width("se.1", 12);
        assert!(ascii >= 4 * 7, "{ascii}");
        let cjk = text_width("いない", 12);
        assert!(cjk >= 3 * 12, "{cjk}");
        assert!(text_width("あ", 12) > text_width("a", 12));
    }
}
