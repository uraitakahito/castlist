//! 系図の並べ方（純な計算。DB に触らない）: 点（箱の幅と並べる順の鍵）と線から、箱の置き場と線の道を出す。
//!
//! - 上から下への木。起こした（[`Kind::Launched`]）線の元を親として上に、先を子として下に置く。
//! - 根は、組の中に起こした者がいないアクター（人の根など）。
//! - 兄弟は、並べる順の鍵（初めの登場の時刻）の順に左から。
//! - 継いだ（[`Kind::Continued`]）先は、起こした線が無ければ、元のすぐ右の兄弟に置く（元に親が無ければ、元の隣の根）。
//! - 部分木ごとに横の幅を取って並べるので、箱は重ならない。同じ入力からは、いつも同じ答え。

/// 箱の高さ（3 行）。
pub const BOX_H: u32 = 58;
/// 兄弟の箱の間（横）。
pub const H_GAP: u32 = 24;
/// 段の間（縦）。
pub const V_GAP: u32 = 48;
/// 図の縁の余白。
pub const MARGIN: u32 = 16;

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
pub fn text_width(s: &str, px: u32) -> u32 {
    // 百分の一の字で数える（小数を使わない）。
    let hundredths: u32 = s.chars().map(|c| if c.is_ascii() { 62 } else { 100 }).sum();
    (hundredths * px).div_ceil(100)
}

/// 点と線を並べる。
pub fn layout(nodes: &[Node], edges: &[Edge]) -> Layout {
    let n = nodes.len();
    if n == 0 {
        return Layout::default();
    }
    let edges: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.from < n && e.to < n && e.from != e.to)
        .collect();
    let up = uplinks(n, &edges);

    // 兄弟は（並べる順の鍵・添字）の順。
    let by_order = |list: &mut Vec<usize>| {
        list.sort_by(|&a, &b| (&nodes[a].order, a).cmp(&(&nodes[b].order, b)))
    };
    let mut kids = vec![Vec::new(); n];
    let mut next = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for (i, u) in up.iter().enumerate() {
        match u {
            Some((p, Kind::Launched)) => kids[*p].push(i),
            Some((p, Kind::Continued)) => next[*p].push(i),
            None => roots.push(i),
        }
    }
    for list in kids.iter_mut().chain(next.iter_mut()) {
        by_order(list);
    }
    by_order(&mut roots);
    // 並べる上の子: 起こした子を順に、それぞれのすぐ右に、継いだ後（その後も）を足す。
    let children: Vec<Vec<usize>> = kids.iter().map(|k| with_successors(k, &next)).collect();
    let top = with_successors(&roots, &next);

    // 段（深さ）と、上から順の並び。
    let mut depth = vec![0_u32; n];
    let mut preorder = Vec::with_capacity(n);
    let mut stack: Vec<usize> = top.iter().rev().copied().collect();
    while let Some(i) = stack.pop() {
        preorder.push(i);
        for &c in children[i].iter().rev() {
            depth[c] = depth[i] + 1;
            stack.push(c);
        }
    }

    // 部分木の幅: 自分の箱と、子の部分木を並べた幅の、広い方。
    let row = |list: &[usize], span: &[u32]| -> u32 {
        let gaps = u32::try_from(list.len().saturating_sub(1)).unwrap_or(u32::MAX);
        list.iter().map(|&c| span[c]).sum::<u32>() + H_GAP * gaps
    };
    let mut span = vec![0_u32; n];
    for &i in preorder.iter().rev() {
        span[i] = nodes[i].width.max(row(&children[i], &span));
    }

    // 部分木ごとに左の端を決め、箱は部分木の真ん中、子の並びも部分木の真ん中に置く。
    let mut left = vec![0_u32; n];
    let mut cursor = MARGIN;
    for &t in &top {
        left[t] = cursor;
        cursor += span[t] + H_GAP;
    }
    let mut boxes = vec![Rect::default(); n];
    for &i in &preorder {
        boxes[i] = Rect {
            x: left[i] + (span[i] - nodes[i].width) / 2,
            y: MARGIN + depth[i] * (BOX_H + V_GAP),
            w: nodes[i].width,
            h: BOX_H,
        };
        let mut x = left[i] + (span[i] - row(&children[i], &span)) / 2;
        for &c in &children[i] {
            left[c] = x;
            x += span[c] + H_GAP;
        }
    }
    let deepest = depth.iter().copied().max().unwrap_or(0);
    let lines = edges
        .iter()
        .map(|e| Line {
            from: e.from,
            to: e.to,
            kind: e.kind,
            d: path(e, &boxes),
        })
        .collect();
    Layout {
        width: cursor - H_GAP + MARGIN,
        height: MARGIN * 2 + (deepest + 1) * BOX_H + deepest * V_GAP,
        boxes,
        lines,
    }
}

/// 点ごとの、並べる時の上（起こした者。無ければ継がせた者）。どちらも初めの線を使う。輪になれば切る。
fn uplinks(n: usize, edges: &[&Edge]) -> Vec<Option<(usize, Kind)>> {
    let mut up: Vec<Option<(usize, Kind)>> = vec![None; n];
    for kind in [Kind::Launched, Kind::Continued] {
        for e in edges.iter().filter(|e| e.kind == kind) {
            if up[e.to].is_none() {
                up[e.to] = Some((e.from, kind));
            }
        }
    }
    // 上を辿り、辿っている道に戻れば、そこで切る（0: まだ・1: 辿っている・2: 済み）。
    let mut state = vec![0_u8; n];
    for start in 0..n {
        let mut path = Vec::new();
        let mut i = start;
        while state[i] == 0 {
            state[i] = 1;
            path.push(i);
            match up[i] {
                Some((p, _)) if state[p] == 1 => {
                    up[i] = None;
                    break;
                }
                Some((p, _)) => i = p,
                None => break,
            }
        }
        for i in path {
            state[i] = 2;
        }
    }
    up
}

/// 並びの点のそれぞれのすぐ後に、その点を継いだ後（さらにその後も）を差し込む。
fn with_successors(list: &[usize], next: &[Vec<usize>]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack: Vec<usize> = list.iter().rev().copied().collect();
    while let Some(i) = stack.pop() {
        out.push(i);
        stack.extend(next[i].iter().rev());
    }
    out
}

/// 線の道（SVG の path の `d`）。
/// - 下の段へ: 元の箱の下の真ん中から、段の間で折れて、先の箱の上の真ん中へ。
/// - 同じ段で、間にほかの箱が無い: 元の箱の右（左）の真ん中から、先の箱の左（右）の真ん中へ。
/// - 同じ段で、間に箱がある: 段の下をくぐる弧。
/// - 上の段へ（輪を切った線など）: 箱の真ん中どうしを結ぶ。
fn path(e: &Edge, boxes: &[Rect]) -> String {
    let (a, b) = (&boxes[e.from], &boxes[e.to]);
    let (ax, bx) = (a.x + a.w / 2, b.x + b.w / 2);
    if b.y > a.y {
        let bend = b.y - V_GAP / 2;
        return format!("M{ax},{} V{bend} H{bx} V{}", a.y + a.h, b.y);
    }
    if b.y == a.y {
        let (lo, hi) = if a.x < b.x { (a, b) } else { (b, a) };
        let between = boxes
            .iter()
            .any(|o| o.y == a.y && o.x >= lo.x + lo.w && o.x + o.w <= hi.x);
        let mid = a.y + a.h / 2;
        if !between {
            let (from, to) = if a.x < b.x {
                (a.x + a.w, b.x)
            } else {
                (a.x, b.x + b.w)
            };
            return format!("M{from},{mid} H{to}");
        }
        let (bottom, dip) = (a.y + a.h, a.y + a.h + V_GAP * 2 / 5);
        return format!("M{ax},{bottom} C{ax},{dip} {bx},{dip} {bx},{bottom}");
    }
    format!("M{ax},{} L{bx},{}", a.y + a.h / 2, b.y + b.h / 2)
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
