//! ネイティブ設定ウィンドウのレイアウト計算（OS 非依存の純粋関数）。
//!
//! 寸法は 96 DPI（100%）を基準とした dp で持ち、実際の DPI で px にする（[`scale`]）。
//! 余白は 4dp を単位とするグリッド（4 / 8 / 12 / 16 / 24 / 32）に揃える。
//! 文字の折り返しの高さはウィンドウ側が実測して渡す（ここは文字を測らない）。
//!
//! 画面の構成（クライアント領域）:
//!
//! ```text
//! +----------+-+--------------------------------------+
//! | ブランド   |R| パネル（見出し・項目。縦スクロール）      |
//! | サイドバー |u|                                      |
//! |  (list)  |l|                                      |
//! +----------+-+--------------------------------------+  <- フッターの区切り線
//! | [詳細設定] 状態表示（ステータス）            [閉じる] |
//! +-----------------------------------------------------+
//! ```

/// 長方形（クライアント座標。px）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// 左
    pub x: i32,
    /// 上
    pub y: i32,
    /// 幅
    pub w: i32,
    /// 高さ
    pub h: i32,
}

/// dp → px（四捨五入）
pub fn scale(dp: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    i32::try_from((i64::from(dp) * dpi + 48).div_euclid(96)).unwrap_or(i32::MAX)
}

/// px → dp（[`scale`] の逆。四捨五入）。ウィンドウの大きさを DPI によらない形で保存するのに使う
pub fn unscale(px: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    i32::try_from((i64::from(px) * 96 + dpi / 2).div_euclid(dpi)).unwrap_or(i32::MAX)
}

/// 最小のクライアント領域の幅（dp）
pub const MIN_CLIENT_W: i32 = 840;
/// 最小のクライアント領域の高さ（dp）
pub const MIN_CLIENT_H: i32 = 480;
/// 初期のクライアント領域の幅（dp）
pub const INITIAL_CLIENT_W: i32 = 960;
/// 初期のクライアント領域の高さ（dp）
pub const INITIAL_CLIENT_H: i32 = 680;

/// 区切り線の太さ（px。DPI によらず 1px。オーナードローで描く）
pub const RULE_PX: i32 = 1;

/// サイドバーの幅
const SIDEBAR_W: i32 = 232;
/// サイドバー上部のブランド領域（ロゴ + ワードマーク）の高さ。一覧はこの下から始まる
const BRAND_H: i32 = 80;
/// ブランド領域のロゴの一辺
const BRAND_ICON: i32 = 32;
/// ブランド領域の左余白
const BRAND_PAD: i32 = 20;
/// ロゴと文字の間隔
const BRAND_GAP: i32 = 12;
/// ブランドのワードマーク（Nagi）の行の高さ
const BRAND_TITLE_H: i32 = 28;
/// ブランドの補足（設定・版）の行の高さ
const BRAND_CAPTION_H: i32 = 20;
/// ページの見出しの上に引くアクセントの線の幅・高さ・見出しまでの間隔
pub const HERO_BAR_W: i32 = 40;
/// ページの見出しの上のアクセントの線の高さ
pub const HERO_BAR_H: i32 = 4;
/// アクセントの線と見出しの間隔
const HERO_BAR_GAP: i32 = 12;
/// 節の見出しの左に付ける印の幅
pub const SECTION_MARK_W: i32 = 4;
/// 節の見出しの印と文字の間隔
const SECTION_MARK_GAP: i32 = 10;
/// 節の見出しの印の最大の高さ
const SECTION_MARK_H: i32 = 18;
/// サイドバー 1 項目の高さ
pub const SIDEBAR_ITEM_H: i32 = 40;
/// サイドバー項目の外側の余白（選択の塗りと一覧の端の間）
pub const SIDEBAR_ITEM_INSET: i32 = 8;
/// サイドバー項目の文字の左余白（選択の塗りの左端から）
pub const SIDEBAR_TEXT_PAD: i32 = 12;
/// サイドバー項目のアイコン欄の幅（アイコンの文字の中心をこの幅の中央に置く）
pub const SIDEBAR_ICON_W: i32 = 20;
/// アイコンと文字の間隔
pub const SIDEBAR_ICON_GAP: i32 = 8;
/// 選択中の項目の左端に付けるアクセントの印の幅・高さ
pub const SIDEBAR_BAR_W: i32 = 4;
/// 選択中の項目の左端に付けるアクセントの印の高さ
pub const SIDEBAR_BAR_H: i32 = 16;
/// フッター（区切り線を含む）の高さ
const FOOTER_H: i32 = 64;
/// フッターの左右の余白
const FOOTER_PAD: i32 = 16;
/// 「閉じる」ボタンの幅
const CLOSE_W: i32 = 112;
/// フッターの部品の間隔
const FOOTER_GAP: i32 = 16;
/// パネル内の上下左右の余白
const PANEL_PAD: i32 = 24;
/// 行の中の段の間隔
const LINE_GAP: i32 = 4;
/// カード（項目グループの枠）の内側の余白（上下左右）
pub const CARD_PAD: i32 = 16;
/// 入力欄・ボタンの高さ
pub const CONTROL_H: i32 = 32;
/// 入力欄・コンボボックスの幅（すべての項目で同じ）
const CONTROL_W: i32 = 280;
/// 「既定に戻す」の幅
const RESET_W: i32 = 120;
/// 入力欄と範囲の案内などの間隔
const HINT_GAP: i32 = 12;
/// 動作ボタン（更新を確認・フォルダを開く等）の幅（すべて同じ）
pub const BUTTON_W: i32 = 224;
/// ボタン同士の横の間隔
const BUTTON_GAP: i32 = 8;
const SCROLL_STEP: i32 = 48;

/// 行と行の間の空き（前の行との関係で決まる）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gap {
    /// 項目と項目の間
    #[default]
    Row,
    /// 前の行に付属する行（見出しの下の説明・ボタン、ボタンの下のボタン）
    Tight,
    /// 節（グループ）の前
    Section,
    /// ページの見出しの直後
    AfterHeading,
}

/// 空きの大きさ（px）
pub fn gap_px(gap: Gap, dpi: u32) -> i32 {
    scale(
        match gap {
            Gap::Row => 24,
            Gap::Tight => 8,
            Gap::Section => 32,
            Gap::AfterHeading => 16,
        },
        dpi,
    )
}

/// 行の種類（行と行の間の空きを決めるのに使う）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// ページの見出し
    Heading,
    /// 節の見出し
    Section,
    /// ページの見出しの下の 1 行の説明
    Subtitle,
    /// 設定項目
    Field,
    /// 本文
    Text,
    /// 説明
    Desc,
    /// エラー
    Error,
    /// ボタンの行
    Buttons,
}

/// 前の行 `prev`（先頭なら `None`）との関係から、`this` の前の空きを決める。
/// 見出しの直後・節の前・付属する行（見出しの下の説明やボタン、ボタンの連なり）を区別する
pub fn gap_between(prev: Option<RowKind>, this: RowKind) -> Gap {
    use RowKind::{Buttons, Desc, Error, Heading, Section, Subtitle, Text};
    match (prev, this) {
        (None, _) => Gap::Row,
        (Some(Heading), Subtitle) => Gap::Tight,
        (Some(Heading | Subtitle), _) => Gap::AfterHeading,
        (_, Section) => Gap::Section,
        (Some(Section), _) => Gap::Tight,
        (Some(Text | Desc), Desc | Error) => Gap::Tight,
        (Some(Text | Desc | Error | Buttons), Buttons) => Gap::Tight,
        _ => Gap::Row,
    }
}

/// その種類の行をカード（薄い枠 + 角丸の背景）の中に置くか。見出し・節の見出し・見出しの下の説明はカードの外
pub fn row_in_card(kind: RowKind) -> bool {
    !matches!(
        kind,
        RowKind::Heading | RowKind::Section | RowKind::Subtitle
    )
}

/// ウィンドウ全体の部品の配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    /// サイドバー上部のブランド領域（ロゴ + ワードマーク）
    pub brand: Rect,
    /// 左のサイドバー（ページ一覧。ブランド領域の下）
    pub list: Rect,
    /// サイドバーとパネルの間の縦線
    pub sidebar_rule: Rect,
    /// 右のスクロールパネル
    pub panel: Rect,
    /// フッターの上の横線
    pub footer_rule: Rect,
    /// 「詳細設定を表示」（サイドバーの列の下）
    pub detail_check: Rect,
    /// ステータス行（パネルの内容の左端に揃える）
    pub status: Rect,
    /// 「閉じる」（右下）
    pub close: Rect,
}

/// クライアント領域（px）から部品の配置を求める。小さすぎるときも幅・高さは 0 以上
pub fn frame_layout(client_w: i32, client_h: i32, dpi: u32) -> FrameLayout {
    let client_w = client_w.max(0);
    let client_h = client_h.max(0);
    let side_w = scale(SIDEBAR_W, dpi).min(client_w);
    let footer_h = scale(FOOTER_H, dpi).min(client_h);
    let footer_y = client_h - footer_h;
    let top = scale(BRAND_H, dpi).min(footer_y);
    let rule = RULE_PX.min(client_w - side_w).max(0);
    let panel_x = side_w + rule;
    let ctl_h = control_height(dpi);
    // フッターの区切り線の下で、部品を縦中央に置く
    let inner_h = (footer_h - RULE_PX).max(0);
    let ctl_y = footer_y + RULE_PX + ((inner_h - ctl_h) / 2).max(0);
    let pad = scale(FOOTER_PAD, dpi);
    let close_w = scale(CLOSE_W, dpi).min((client_w - 2 * pad).max(0));
    let close_x = (client_w - pad - close_w).max(0);
    let status_x = (panel_x + scale(PANEL_PAD, dpi)).min(close_x);
    let status_w = (close_x - scale(FOOTER_GAP, dpi) - status_x).max(0);
    FrameLayout {
        brand: Rect {
            x: 0,
            y: 0,
            w: side_w,
            h: top,
        },
        list: Rect {
            x: 0,
            y: top,
            w: side_w,
            h: (footer_y - top).max(0),
        },
        sidebar_rule: Rect {
            x: side_w,
            y: 0,
            w: rule,
            h: footer_y,
        },
        panel: Rect {
            x: panel_x,
            y: 0,
            w: (client_w - panel_x).max(0),
            h: footer_y,
        },
        footer_rule: Rect {
            x: 0,
            y: footer_y,
            w: client_w,
            h: RULE_PX.min(footer_h),
        },
        detail_check: Rect {
            x: pad.min(side_w),
            y: ctl_y,
            w: (side_w - pad - scale(8, dpi)).max(0),
            h: ctl_h,
        },
        status: Rect {
            x: status_x,
            y: ctl_y,
            w: status_w,
            h: ctl_h,
        },
        close: Rect {
            x: close_x,
            y: ctl_y,
            w: close_w,
            h: ctl_h,
        },
    }
}

/// ブランド領域の部品の位置（px。`brand` 内の座標ではなく `brand` と同じ座標系）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrandParts {
    /// ロゴ（正方形）
    pub icon: Rect,
    /// ワードマーク（Nagi）の行
    pub title: Rect,
    /// 補足（設定・版）の行
    pub caption: Rect,
}

/// ブランド領域 `brand` から、ロゴ・ワードマーク・補足の位置を求める。ロゴは縦中央、文字の 2 行は
/// 縦中央に寄せて積む
pub fn brand_parts(brand: Rect, dpi: u32) -> BrandParts {
    let icon_px = scale(BRAND_ICON, dpi);
    let x = brand.x + scale(BRAND_PAD, dpi);
    let icon = Rect {
        x,
        y: brand.y + (brand.h - icon_px) / 2,
        w: icon_px,
        h: icon_px,
    };
    let text_x = icon.x + icon.w + scale(BRAND_GAP, dpi);
    let (th, ch) = (scale(BRAND_TITLE_H, dpi), scale(BRAND_CAPTION_H, dpi));
    let top = brand.y + (brand.h - th - ch) / 2;
    let w = (brand.x + brand.w - text_x - scale(8, dpi)).max(0);
    BrandParts {
        icon,
        title: Rect {
            x: text_x,
            y: top,
            w,
            h: th,
        },
        caption: Rect {
            x: text_x,
            y: top + th,
            w,
            h: ch,
        },
    }
}

/// パネル内の列の位置（パネルのクライアント幅から。スクロールバーを除いた幅を渡す）
///
/// すべての項目で、入力欄は同じ左端・同じ幅、「既定に戻す」は同じ右端の列に揃える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// カードの左端（ページの見出し・節の見出しもここに揃える）
    pub card_x: i32,
    /// カードの幅（見出しの折り返し幅）
    pub card_w: i32,
    /// 節の見出しの文字の左端（左に印を付けるため、カードの左端より内側）
    pub section_x: i32,
    /// 節の見出しの折り返し幅
    pub section_w: i32,
    /// 内容の左端（カードの内側）
    pub x: i32,
    /// 内容の幅（ラベル・説明・エラーの折り返し幅。カードの内側）
    pub width: i32,
    /// 入力欄の幅
    pub control_w: i32,
    /// 範囲の案内の左端（入力欄の右）
    pub hint_x: i32,
    /// 範囲の案内の幅（「既定に戻す」の左まで）
    pub hint_w: i32,
    /// 「既定に戻す」の左端
    pub reset_x: i32,
    /// 「既定に戻す」の幅
    pub reset_w: i32,
    /// チェックボックスの幅（「既定に戻す」の左まで）
    pub check_w: i32,
}

/// 列を求める
pub fn columns(panel_client_w: i32, dpi: u32) -> Columns {
    let outer = scale(PANEL_PAD, dpi);
    let card_pad = scale(CARD_PAD, dpi);
    let pad = outer + card_pad;
    let width = (panel_client_w - 2 * pad).max(scale(120, dpi));
    let control_w = scale(CONTROL_W, dpi).min(width);
    let reset_w = scale(RESET_W, dpi);
    let gap = scale(HINT_GAP, dpi);
    let hint_x = pad + control_w + gap;
    let reset_x = (pad + width - reset_w).max(hint_x);
    let card_w = width + 2 * card_pad;
    let section_off = scale(SECTION_MARK_W, dpi) + scale(SECTION_MARK_GAP, dpi);
    Columns {
        card_x: outer,
        card_w,
        section_x: outer + section_off,
        section_w: (card_w - section_off).max(0),
        x: pad,
        width,
        control_w,
        hint_x,
        hint_w: (reset_x - gap - hint_x).max(0),
        reset_x,
        reset_w,
        check_w: (reset_x - gap - pad).max(scale(60, dpi)),
    }
}

/// 行の見た目の役割
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowStyle {
    /// 通常の行
    #[default]
    Plain,
    /// ページの見出し（先頭の行。上にアクセントの線を引く）
    Hero,
    /// 節の見出し（左にアクセントの印を付ける）
    Section,
}

/// 1 行（項目 1 つ）の部品の高さ（px。無ければ 0）。折り返しの高さはウィンドウ側が測って渡す
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowSpec {
    /// 行の見た目の役割（見出し・節の見出しには装飾の分の余白・印の位置が付く）
    pub style: RowStyle,
    /// 前の行との間の空き
    pub gap: Gap,
    /// カードの中の行か（連続する行が 1 枚のカードになる）
    pub in_card: bool,
    /// ラベル・見出し（チェックボックスのときはそれ全体）の高さ
    pub title_h: i32,
    /// 説明の高さ
    pub desc_h: i32,
    /// 入力欄・ボタンの行の高さ
    pub control_h: i32,
    /// エラーの高さ
    pub error_h: i32,
}

/// 1 行の配置（内容座標。スクロール前の y）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowLayout {
    /// 行の上端
    pub top: i32,
    /// ラベルの上端
    pub title_y: i32,
    /// 説明の上端
    pub desc_y: i32,
    /// 入力欄の行の上端
    pub control_y: i32,
    /// エラーの上端
    pub error_y: i32,
    /// 行の高さ（下の余白を含まない）
    pub height: i32,
}

/// カード 1 枚の縦の範囲（内容座標。スクロール前の y）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardSpan {
    /// 上端
    pub top: i32,
    /// 下端（含まない）
    pub bottom: i32,
}

/// パネル全体の配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelLayout {
    /// 各行
    pub rows: Vec<RowLayout>,
    /// カード（`in_card` の行が連続する範囲ごとに 1 枚）
    pub cards: Vec<CardSpan>,
    /// カードの中の項目と項目の間に引く薄い区切りの y（項目の間の空きの中央）
    pub dividers: Vec<i32>,
    /// ページの見出しの上のアクセントの線の上端（[`RowStyle::Hero`] の行があるときだけ）
    pub hero_bar_y: Option<i32>,
    /// 節の見出しの印の縦の範囲（[`RowStyle::Section`] の行ごと。内容座標）
    pub markers: Vec<CardSpan>,
    /// 内容の高さ（上下の余白を含む。スクロール範囲の元）
    pub content_height: i32,
}

/// 行を縦に積む。部品が無い（高さ 0 の）段は詰め、段の間に [`LINE_GAP`] を入れる。
/// `in_card` の行が連続する範囲は 1 枚のカードにし、上下に [`CARD_PAD`] の余白を取る
/// （前の行との間の空きは、カードの外枠までの空きになる）
pub fn layout_rows(rows: &[RowSpec], dpi: u32) -> PanelLayout {
    let pad = scale(PANEL_PAD, dpi);
    let card_pad = scale(CARD_PAD, dpi);
    let line_gap = scale(LINE_GAP, dpi);
    let mut y = pad;
    let mut out = Vec::with_capacity(rows.len());
    let mut cards: Vec<CardSpan> = Vec::new();
    let mut dividers = Vec::new();
    let mut markers = Vec::new();
    let mut hero_bar_y = None;
    let mut open: Option<i32> = None;
    for (n, spec) in rows.iter().enumerate() {
        if n == 0 && spec.style == RowStyle::Hero {
            // 見出しの上にアクセントの線を置く分だけ下げる
            hero_bar_y = Some(y);
            y += scale(HERO_BAR_H, dpi) + scale(HERO_BAR_GAP, dpi);
        }
        if n > 0 {
            let gap = gap_px(spec.gap, dpi);
            if open.is_some() && spec.in_card {
                // 同じカードの中の項目の間: 項目と項目の間だけ区切りを引く
                if spec.gap == Gap::Row {
                    dividers.push(y + gap / 2);
                }
            } else if let Some(top) = open.take() {
                y += card_pad;
                cards.push(CardSpan { top, bottom: y });
            }
            y += gap;
        }
        if spec.in_card && open.is_none() {
            open = Some(y);
            y += card_pad;
        }
        let top = y;
        let mut place = |h: i32| -> i32 {
            if h <= 0 {
                return y;
            }
            let at = y;
            y += h + line_gap;
            at
        };
        let title_y = place(spec.title_h);
        if spec.style == RowStyle::Section && spec.title_h > 0 {
            // 印は文字の行の中央に、最大 SECTION_MARK_H の高さで置く
            let h = spec.title_h.min(scale(SECTION_MARK_H, dpi));
            let top = title_y + (spec.title_h - h) / 2;
            markers.push(CardSpan {
                top,
                bottom: top + h,
            });
        }
        let desc_y = place(spec.desc_h);
        let control_y = place(spec.control_h);
        let error_y = place(spec.error_h);
        let any = [spec.title_h, spec.desc_h, spec.control_h, spec.error_h]
            .iter()
            .any(|h| *h > 0);
        if any {
            y -= line_gap; // 最後の段の下の隙間は行の高さに含めない
        }
        out.push(RowLayout {
            top,
            title_y,
            desc_y,
            control_y,
            error_y,
            height: y - top,
        });
    }
    if let Some(top) = open {
        y += card_pad;
        cards.push(CardSpan { top, bottom: y });
    }
    PanelLayout {
        rows: out,
        cards,
        dividers,
        hero_bar_y,
        markers,
        content_height: y + pad,
    }
}

/// ページの見出しの上のアクセントの線（`y` は [`PanelLayout::hero_bar_y`]）
pub fn hero_bar_rect(cols: &Columns, y: i32, dpi: u32) -> Rect {
    Rect {
        x: cols.card_x,
        y,
        w: scale(HERO_BAR_W, dpi),
        h: scale(HERO_BAR_H, dpi),
    }
}

/// 節の見出しの左の印（`span` は [`PanelLayout::markers`] の 1 つ）
pub fn section_marker_rect(cols: &Columns, span: CardSpan, dpi: u32) -> Rect {
    Rect {
        x: cols.card_x,
        y: span.top,
        w: scale(SECTION_MARK_W, dpi),
        h: span.bottom - span.top,
    }
}

/// スクロール位置の最大値
pub fn max_scroll(content_height: i32, viewport_height: i32) -> i32 {
    (content_height - viewport_height).max(0)
}

/// スクロール位置を範囲内にする
pub fn clamp_scroll(pos: i32, content_height: i32, viewport_height: i32) -> i32 {
    pos.clamp(0, max_scroll(content_height, viewport_height))
}

/// マウスホイール 1 ノッチ分の移動量（px）
pub fn wheel_step(dpi: u32) -> i32 {
    scale(SCROLL_STEP, dpi)
}

/// 内容座標の `[top, bottom)` が見えるように、スクロール位置を最小限動かす
pub fn scroll_into_view(
    scroll: i32,
    viewport_height: i32,
    top: i32,
    bottom: i32,
    content_height: i32,
    margin: i32,
) -> i32 {
    let mut pos = scroll;
    if top - margin < pos {
        pos = top - margin;
    } else if bottom + margin > pos + viewport_height {
        pos = bottom + margin - viewport_height;
    }
    clamp_scroll(pos, content_height, viewport_height)
}

/// ウィンドウの最小サイズ（クライアント領域。px）
pub fn min_client_size(dpi: u32) -> (i32, i32) {
    (scale(MIN_CLIENT_W, dpi), scale(MIN_CLIENT_H, dpi))
}

/// 初期のクライアント領域のサイズ（px）
pub fn initial_client_size(dpi: u32) -> (i32, i32) {
    (scale(INITIAL_CLIENT_W, dpi), scale(INITIAL_CLIENT_H, dpi))
}

/// 入力欄・ボタンの高さ（px）
pub fn control_height(dpi: u32) -> i32 {
    scale(CONTROL_H, dpi)
}

/// ボタンを横に並べる（左から。幅は `widths_dp`、間隔は一定）。各ボタンの `(x, w)`
pub fn button_row(origin_x: i32, widths_dp: &[i32], dpi: u32) -> Vec<(i32, i32)> {
    let gap = scale(BUTTON_GAP, dpi);
    let mut x = origin_x;
    widths_dp
        .iter()
        .map(|dp| {
            let w = scale(*dp, dpi);
            let out = (x, w);
            x += w + gap;
            out
        })
        .collect()
}

/// サイドバー項目の高さ（px）
pub fn sidebar_item_height(dpi: u32) -> i32 {
    scale(SIDEBAR_ITEM_H, dpi)
}

/// サイドバー 1 項目の部品の位置（px）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarParts {
    /// 選択・ホバーの丸みのある帯
    pub fill: Rect,
    /// 選択中だけ左端に付けるアクセントの印
    pub bar: Rect,
    /// アイコン欄（アイコンの文字をこの中央に置く）
    pub icon: Rect,
    /// アイコンがあるときの文字の左端
    pub text_x: i32,
    /// アイコンが無いときの文字の左端
    pub text_x_no_icon: i32,
}

/// サイドバー項目の描画領域 `item`（項目の全体）から、帯・アクセントの印・アイコン・文字の位置を求める
pub fn sidebar_item_parts(item: Rect, dpi: u32) -> SidebarParts {
    let inset = scale(SIDEBAR_ITEM_INSET, dpi);
    let v = scale(2, dpi);
    let fill = Rect {
        x: item.x + inset,
        y: item.y + v,
        w: (item.w - 2 * inset).max(0),
        h: (item.h - 2 * v).max(0),
    };
    let bar_h = scale(SIDEBAR_BAR_H, dpi).min(fill.h);
    let bar = Rect {
        x: fill.x,
        y: fill.y + (fill.h - bar_h) / 2,
        w: scale(SIDEBAR_BAR_W, dpi).min(fill.w),
        h: bar_h,
    };
    let text_x_no_icon = fill.x + scale(SIDEBAR_TEXT_PAD, dpi);
    let icon = Rect {
        x: text_x_no_icon,
        y: fill.y,
        w: scale(SIDEBAR_ICON_W, dpi),
        h: fill.h,
    };
    SidebarParts {
        fill,
        bar,
        icon,
        text_x: icon.x + icon.w + scale(SIDEBAR_ICON_GAP, dpi),
        text_x_no_icon,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DPIS: [u32; 5] = [96, 120, 144, 192, 240];

    #[test]
    fn scale_rounds_and_is_monotonic() {
        assert_eq!(scale(100, 96), 100);
        assert_eq!(scale(100, 144), 150);
        assert_eq!(scale(100, 192), 200);
        assert_eq!(scale(0, 144), 0);
        assert_eq!(scale(1, 96), 1);
        assert_eq!(scale(3, 144), 5); // 4.5 → 5
        let mut last = 0;
        for dpi in DPIS {
            let v = scale(37, dpi);
            assert!(v >= last);
            last = v;
        }
        let _ = scale(10, 0); // DPI 0 でも落ちない
    }

    #[test]
    fn unscale_inverts_scale() {
        for dpi in DPIS {
            for dp in [0, 1, 7, 100, 760, 880, 1000] {
                assert_eq!(unscale(scale(dp, dpi), dpi), dp, "dp={dp} dpi={dpi}");
            }
        }
        let _ = unscale(10, 0);
    }

    #[test]
    fn scroll_range_and_clamping() {
        assert_eq!(max_scroll(500, 300), 200);
        assert_eq!(max_scroll(200, 300), 0);
        assert_eq!(clamp_scroll(-5, 500, 300), 0);
        assert_eq!(clamp_scroll(250, 500, 300), 200);
        assert_eq!(clamp_scroll(100, 500, 300), 100);
        for dpi in DPIS {
            let layout = layout_rows(&sample_rows(dpi), dpi);
            let viewport = scale(100, dpi);
            let max = max_scroll(layout.content_height, viewport);
            assert_eq!(max, layout.content_height - viewport);
            assert!(wheel_step(dpi) > 0);
        }
        assert!(wheel_step(192) > wheel_step(96));
    }

    #[test]
    fn scroll_into_view_moves_the_minimum() {
        // 見えているなら動かない
        assert_eq!(scroll_into_view(100, 300, 150, 200, 1000, 8), 100);
        // 下に隠れている → 下端が見える位置へ
        assert_eq!(scroll_into_view(0, 300, 400, 430, 1000, 8), 138);
        // 上に隠れている → 上端が見える位置へ
        assert_eq!(scroll_into_view(500, 300, 300, 330, 1000, 8), 292);
        // 範囲を超えない
        assert_eq!(scroll_into_view(0, 300, 990, 1000, 1000, 8), 700);
        assert_eq!(scroll_into_view(50, 300, 0, 20, 1000, 8), 0);
    }

    #[test]
    fn button_row_places_buttons_left_to_right() {
        let b = button_row(16, &[100, 120, 90], 144);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0], (16, 150));
        assert!(b[1].0 >= b[0].0 + b[0].1);
        assert!(b[2].0 >= b[1].0 + b[1].1);
        assert_eq!(b[1].1, 180);
    }

    #[test]
    fn initial_size_is_not_smaller_than_the_minimum() {
        for dpi in DPIS {
            let (iw, ih) = initial_client_size(dpi);
            let (mw, mh) = min_client_size(dpi);
            assert!(iw >= mw && ih >= mh);
        }
    }

    #[test]
    fn frame_regions_fit_inside_the_client_area() {
        for dpi in DPIS {
            let (min_w, min_h) = min_client_size(dpi);
            for (w, h) in [(min_w, min_h), (scale(900, dpi), scale(700, dpi))] {
                let f = frame_layout(w, h, dpi);
                for r in [
                    f.brand,
                    f.list,
                    f.sidebar_rule,
                    f.panel,
                    f.footer_rule,
                    f.detail_check,
                    f.status,
                    f.close,
                ] {
                    assert!(r.w >= 0 && r.h >= 0, "{r:?}");
                    assert!(r.x >= 0 && r.x + r.w <= w, "{r:?} w={w}");
                    assert!(r.y >= 0 && r.y + r.h <= h, "{r:?} h={h}");
                }
                // 横: サイドバー | 縦線 | パネル
                assert_eq!(f.list.x + f.list.w, f.sidebar_rule.x);
                assert_eq!(f.sidebar_rule.x + f.sidebar_rule.w, f.panel.x);
                assert_eq!(f.panel.x + f.panel.w, w);
                // 縦: 本体 | 区切り線 | フッター
                assert!(f.panel.y + f.panel.h <= f.footer_rule.y);
                assert!(f.footer_rule.y + f.footer_rule.h <= f.close.y);
                assert!(f.panel.w > scale(400, dpi));
                assert!(f.panel.h > scale(250, dpi));
                // フッター: 詳細設定 < ステータス < 閉じる（重ならない）
                assert!(f.detail_check.x + f.detail_check.w <= f.status.x);
                assert!(f.status.x + f.status.w <= f.close.x);
                assert!(f.status.w > scale(100, dpi));
                // フッターの部品は高さと縦位置が揃う
                assert_eq!(f.detail_check.y, f.status.y);
                assert_eq!(f.status.y, f.close.y);
                assert_eq!(f.close.h, control_height(dpi));
            }
        }
    }

    #[test]
    fn footer_aligns_with_sidebar_and_panel_content() {
        for dpi in DPIS {
            let (w, h) = initial_client_size(dpi);
            let f = frame_layout(w, h, dpi);
            // ステータスの左端 = パネルのカードの左端
            let cols = columns(f.panel.w, dpi);
            assert_eq!(f.status.x, f.panel.x + cols.card_x);
            // 「閉じる」の右端の余白 = 詳細設定の左の余白
            assert_eq!(w - (f.close.x + f.close.w), f.detail_check.x);
            // 詳細設定はサイドバーの列に収まる
            assert!(f.detail_check.x + f.detail_check.w <= f.list.w);
        }
    }

    #[test]
    fn frame_layout_survives_a_tiny_window() {
        for (w, h) in [(10, 10), (0, 0), (-5, -5), (300, 20)] {
            let f = frame_layout(w, h, 96);
            for r in [
                f.brand,
                f.list,
                f.sidebar_rule,
                f.panel,
                f.footer_rule,
                f.detail_check,
                f.status,
                f.close,
            ] {
                assert!(r.w >= 0 && r.h >= 0, "{r:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn spacing_follows_the_4dp_grid() {
        // 余白の dp はすべて 4 の倍数（DPI 100% のとき px と一致する）
        for gap in [Gap::Row, Gap::Tight, Gap::Section, Gap::AfterHeading] {
            assert_eq!(gap_px(gap, 96) % 4, 0, "{gap:?}");
        }
        for dp in [
            SIDEBAR_W,
            BRAND_H,
            BRAND_ICON,
            BRAND_GAP,
            BRAND_PAD,
            HERO_BAR_W,
            HERO_BAR_H,
            HERO_BAR_GAP,
            SECTION_MARK_W,
            SIDEBAR_ITEM_H,
            SIDEBAR_ITEM_INSET,
            SIDEBAR_TEXT_PAD,
            SIDEBAR_ICON_W,
            SIDEBAR_ICON_GAP,
            SIDEBAR_BAR_W,
            SIDEBAR_BAR_H,
            CARD_PAD,
            FOOTER_H,
            FOOTER_PAD,
            FOOTER_GAP,
            PANEL_PAD,
            LINE_GAP,
            CONTROL_H,
            CONTROL_W,
            RESET_W,
            HINT_GAP,
            BUTTON_W,
            BUTTON_GAP,
            CLOSE_W,
        ] {
            assert_eq!(dp % 4, 0, "{dp}");
        }
        // 間隔の大小: 付属 < 項目 < 節
        for dpi in DPIS {
            assert!(gap_px(Gap::Tight, dpi) < gap_px(Gap::Row, dpi));
            assert!(gap_px(Gap::Row, dpi) < gap_px(Gap::Section, dpi));
            assert!(gap_px(Gap::AfterHeading, dpi) < gap_px(Gap::Row, dpi));
        }
    }

    #[test]
    fn columns_are_ordered_and_fit_the_width() {
        for dpi in DPIS {
            let (min_w, min_h) = min_client_size(dpi);
            let panel_w = frame_layout(min_w, min_h, dpi).panel.w;
            let c = columns(panel_w, dpi);
            assert!(c.card_x > 0);
            assert_eq!(c.x, c.card_x + scale(CARD_PAD, dpi));
            assert_eq!(c.card_w, c.width + 2 * scale(CARD_PAD, dpi));
            assert!(c.card_x + c.card_w <= panel_w);
            assert!(c.x + c.width <= panel_w);
            assert!(c.control_w <= c.width);
            assert!(c.hint_x >= c.x + c.control_w);
            assert!(c.reset_x >= c.hint_x);
            assert!(c.reset_x + c.reset_w <= c.x + c.width + 1);
            assert!(c.hint_x + c.hint_w <= c.reset_x);
            assert!(c.x + c.check_w <= c.reset_x);
            // 最小幅でも案内に最低限の幅がある
            assert!(c.hint_w >= scale(64, dpi), "dpi={dpi} {c:?}");
        }
        // 入力欄の幅はパネルの幅によらず一定（統一）、広いパネルでは案内が広がる
        assert_eq!(columns(900, 96).control_w, columns(600, 96).control_w);
        assert!(columns(900, 96).hint_w > columns(600, 96).hint_w);
        // 「既定に戻す」の右端は常に内容の右端
        for w in [600, 700, 900, 1200] {
            let c = columns(w, 96);
            assert_eq!(c.reset_x + c.reset_w, c.x + c.width);
        }
    }

    fn sample_rows(dpi: u32) -> Vec<RowSpec> {
        let line = scale(16, dpi);
        vec![
            RowSpec {
                title_h: line * 2,
                ..RowSpec::default()
            },
            RowSpec {
                gap: Gap::AfterHeading,
                title_h: line,
                desc_h: line * 2,
                control_h: control_height(dpi),
                ..RowSpec::default()
            },
            RowSpec {
                title_h: line,
                desc_h: line,
                error_h: line,
                ..RowSpec::default()
            },
            RowSpec::default(),
            RowSpec {
                gap: Gap::Section,
                title_h: line,
                ..RowSpec::default()
            },
            RowSpec {
                gap: Gap::Tight,
                control_h: control_height(dpi),
                ..RowSpec::default()
            },
        ]
    }

    #[test]
    fn rows_stack_without_overlap() {
        for dpi in DPIS {
            let rows = sample_rows(dpi);
            let layout = layout_rows(&rows, dpi);
            assert_eq!(layout.rows.len(), rows.len());
            let mut prev_bottom = 0;
            for (spec, row) in rows.iter().zip(&layout.rows) {
                assert!(row.top >= prev_bottom);
                let mut cursor = row.top;
                for (h, y) in [
                    (spec.title_h, row.title_y),
                    (spec.desc_h, row.desc_y),
                    (spec.control_h, row.control_y),
                    (spec.error_h, row.error_y),
                ] {
                    if h > 0 {
                        assert!(y >= cursor, "dpi={dpi}");
                        cursor = y + h;
                    }
                }
                assert_eq!(row.top + row.height, cursor.max(row.top));
                prev_bottom = row.top + row.height;
            }
            assert!(layout.content_height > prev_bottom);
        }
    }

    #[test]
    fn row_gaps_come_from_the_row_that_follows() {
        let dpi = 96;
        let rows = sample_rows(dpi);
        let layout = layout_rows(&rows, dpi);
        for n in 1..rows.len() {
            let prev = layout.rows[n - 1];
            if rows[n].gap == Gap::Row && rows[n - 1] != RowSpec::default() {
                assert_eq!(layout.rows[n].top - (prev.top + prev.height), 24);
            }
        }
        // 見出しの直後・節の前・付属行の間隔
        assert_eq!(
            layout.rows[1].top - (layout.rows[0].top + layout.rows[0].height),
            16
        );
        assert_eq!(
            layout.rows[4].top - (layout.rows[3].top + layout.rows[3].height),
            32
        );
        assert_eq!(
            layout.rows[5].top - (layout.rows[4].top + layout.rows[4].height),
            8
        );
        // 先頭の行の gap は無視され、上の余白だけが入る
        assert_eq!(layout.rows[0].top, 24);
    }

    #[test]
    fn gap_between_rows_by_kind() {
        use RowKind::*;
        assert_eq!(gap_between(None, Heading), Gap::Row);
        assert_eq!(gap_between(Some(Heading), Field), Gap::AfterHeading);
        assert_eq!(gap_between(Some(Heading), Section), Gap::AfterHeading);
        assert_eq!(gap_between(Some(Field), Field), Gap::Row);
        assert_eq!(gap_between(Some(Field), Section), Gap::Section);
        assert_eq!(gap_between(Some(Buttons), Section), Gap::Section);
        assert_eq!(gap_between(Some(Section), Desc), Gap::Tight);
        assert_eq!(gap_between(Some(Section), Buttons), Gap::Tight);
        assert_eq!(gap_between(Some(Desc), Buttons), Gap::Tight);
        assert_eq!(gap_between(Some(Buttons), Buttons), Gap::Tight);
        assert_eq!(gap_between(Some(Text), Error), Gap::Tight);
        // 操作の結果の文言はボタンから離す
        assert_eq!(gap_between(Some(Buttons), Text), Gap::Row);
    }

    #[test]
    fn content_height_grows_with_dpi_and_rows() {
        let mut last = 0;
        for dpi in DPIS {
            let h = layout_rows(&sample_rows(dpi), dpi).content_height;
            assert!(h > last, "dpi={dpi} h={h} last={last}");
            last = h;
        }
        let empty = layout_rows(&[], 96);
        assert_eq!(empty.content_height, 2 * scale(PANEL_PAD, 96));
        let one = layout_rows(&sample_rows(96)[..1], 96).content_height;
        let two = layout_rows(&sample_rows(96)[..2], 96).content_height;
        assert!(two > one);
    }

    #[test]
    fn sidebar_item_fill_stays_inside_the_item() {
        for dpi in DPIS {
            let item = Rect {
                x: 0,
                y: 100,
                w: scale(SIDEBAR_W, dpi),
                h: sidebar_item_height(dpi),
            };
            let p = sidebar_item_parts(item, dpi);
            let fill = p.fill;
            assert!(fill.x > item.x && fill.x + fill.w < item.x + item.w);
            assert!(fill.y > item.y && fill.y + fill.h < item.y + item.h);
            // 印・アイコン・文字は帯の中に収まり、左から印 < アイコン < 文字の順
            assert!(p.bar.x >= fill.x && p.bar.y >= fill.y);
            assert!(p.bar.y + p.bar.h <= fill.y + fill.h);
            assert!(p.icon.x > p.bar.x + p.bar.w);
            assert!(p.icon.x + p.icon.w < p.text_x);
            assert!(p.text_x < fill.x + fill.w);
            assert!(p.text_x_no_icon > fill.x && p.text_x_no_icon <= p.icon.x);
        }
        let p = sidebar_item_parts(Rect::default(), 96);
        assert!(p.fill.w >= 0 && p.fill.h >= 0 && p.bar.h >= 0);
    }

    fn card_rows(dpi: u32) -> Vec<RowSpec> {
        let line = scale(16, dpi);
        let card = |gap| RowSpec {
            gap,
            in_card: true,
            title_h: line,
            control_h: control_height(dpi),
            ..RowSpec::default()
        };
        vec![
            RowSpec {
                title_h: line * 2,
                ..RowSpec::default()
            },
            RowSpec {
                gap: Gap::AfterHeading,
                title_h: line,
                ..RowSpec::default()
            },
            card(Gap::Tight),
            card(Gap::Row),
            card(Gap::Row),
            RowSpec {
                gap: Gap::Section,
                title_h: line,
                ..RowSpec::default()
            },
            card(Gap::Tight),
        ]
    }

    #[test]
    fn consecutive_card_rows_form_one_card_with_dividers() {
        for dpi in DPIS {
            let rows = card_rows(dpi);
            let l = layout_rows(&rows, dpi);
            // カードは節ごとに 1 枚（2 枚）。重ならず、上から順
            assert_eq!(l.cards.len(), 2, "dpi={dpi}");
            assert!(l.cards[0].bottom < l.cards[1].top);
            let pad = scale(CARD_PAD, dpi);
            // 行はカードの余白の内側に収まる
            let first = l.rows[2];
            let last = l.rows[4];
            assert_eq!(first.top - l.cards[0].top, pad);
            assert_eq!(l.cards[0].bottom - (last.top + last.height), pad);
            // 区切りは 3 行の間の 2 か所（節の外・カードの境目には引かない）
            assert_eq!(l.dividers.len(), 2);
            for d in &l.dividers {
                assert!(*d > l.cards[0].top && *d < l.cards[0].bottom);
            }
            assert!(l.dividers[0] < l.dividers[1]);
            // 内容の高さは最後のカードより下
            assert!(l.content_height > l.cards[1].bottom);
            // 見出しの直後の空き = カードの外枠までの空き
            assert_eq!(
                l.cards[0].top - (l.rows[1].top + l.rows[1].height),
                gap_px(Gap::Tight, dpi)
            );
        }
        // カードの行が無ければカードも区切りも無い
        let none = layout_rows(&sample_rows(96), 96);
        assert!(none.cards.is_empty() && none.dividers.is_empty());
    }

    #[test]
    fn only_headings_stay_outside_cards() {
        use RowKind::*;
        for kind in [Field, Text, Desc, Error, Buttons] {
            assert!(row_in_card(kind), "{kind:?}");
        }
        assert!(!row_in_card(Heading) && !row_in_card(Section));
    }
    #[test]
    fn brand_sits_above_the_list_and_its_parts_fit() {
        for dpi in DPIS {
            let (w, h) = initial_client_size(dpi);
            let f = frame_layout(w, h, dpi);
            // ブランド領域は一覧の真上（同じ幅・隙間なし）
            assert_eq!(f.brand.y, 0);
            assert_eq!(f.brand.w, f.list.w);
            assert_eq!(f.brand.y + f.brand.h, f.list.y);
            let p = brand_parts(f.brand, dpi);
            // ロゴ・2 行の文字は領域の中に収まり、重ならない
            for r in [p.icon, p.title, p.caption] {
                assert!(
                    r.x >= f.brand.x && r.x + r.w <= f.brand.x + f.brand.w,
                    "{r:?}"
                );
                assert!(
                    r.y >= f.brand.y && r.y + r.h <= f.brand.y + f.brand.h,
                    "{r:?}"
                );
            }
            assert!(p.icon.x + p.icon.w < p.title.x);
            assert_eq!(p.title.x, p.caption.x);
            assert!(p.title.y + p.title.h <= p.caption.y);
            // ロゴは正方形で縦中央
            assert_eq!(p.icon.w, p.icon.h);
            assert_eq!(
                p.icon.y - f.brand.y,
                f.brand.h - (p.icon.y - f.brand.y) - p.icon.h
            );
            // ブランドの左端は、サイドバー項目の帯の内側（アイコン欄）と揃う
            let item = sidebar_item_parts(
                Rect {
                    x: 0,
                    y: f.list.y,
                    w: f.list.w,
                    h: sidebar_item_height(dpi),
                },
                dpi,
            );
            assert!(p.icon.x >= item.fill.x);
            assert!(p.icon.x <= item.icon.x);
        }
        let p = brand_parts(Rect::default(), 96);
        assert!(p.title.w >= 0 && p.caption.w >= 0);
    }

    #[test]
    fn hero_heading_reserves_room_for_the_accent_bar() {
        for dpi in DPIS {
            let mut rows = sample_rows(dpi);
            let plain = layout_rows(&rows, dpi);
            assert_eq!(plain.hero_bar_y, None);
            rows[0].style = RowStyle::Hero;
            let hero = layout_rows(&rows, dpi);
            let y = hero.hero_bar_y.expect("線の位置");
            let cols = columns(scale(700, dpi), dpi);
            let bar = hero_bar_rect(&cols, y, dpi);
            // 線は見出しの上に収まり、見出しとは重ならない。左端はカードの左端
            assert_eq!(y, scale(PANEL_PAD, dpi));
            assert!(bar.y + bar.h <= hero.rows[0].title_y);
            assert_eq!(bar.x, cols.card_x);
            // 全体はその分だけ下がる
            assert_eq!(
                hero.rows[0].top - plain.rows[0].top,
                scale(HERO_BAR_H, dpi) + scale(HERO_BAR_GAP, dpi)
            );
            // 先頭の行以外の Hero は無視
            let mut later = sample_rows(dpi);
            later[2].style = RowStyle::Hero;
            assert_eq!(layout_rows(&later, dpi).hero_bar_y, None);
        }
    }

    #[test]
    fn section_markers_sit_beside_the_section_title() {
        for dpi in DPIS {
            let mut rows = card_rows(dpi);
            rows[0].style = RowStyle::Hero;
            rows[1].style = RowStyle::Section;
            rows[5].style = RowStyle::Section;
            let l = layout_rows(&rows, dpi);
            assert_eq!(l.markers.len(), 2);
            let cols = columns(scale(700, dpi), dpi);
            for (span, row) in l.markers.iter().zip([l.rows[1], l.rows[5]]) {
                let spec_h = scale(16, dpi);
                // 印は文字の行の中（縦中央）で、印の高さ > 0
                assert!(span.top >= row.title_y && span.bottom <= row.title_y + spec_h);
                assert!(span.bottom > span.top);
                assert_eq!(span.top - row.title_y, row.title_y + spec_h - span.bottom);
                let m = section_marker_rect(&cols, *span, dpi);
                // 印は文字の左（重ならない）で、カードの左端に揃う
                assert_eq!(m.x, cols.card_x);
                assert!(m.x + m.w < cols.section_x);
            }
            assert!(l.markers[0].bottom < l.markers[1].top);
            // 節の文字の幅はカードの幅から印の分だけ狭い
            assert_eq!(cols.section_x + cols.section_w, cols.card_x + cols.card_w);
        }
    }

    #[test]
    fn subtitle_hugs_the_heading_and_stays_outside_cards() {
        use RowKind::*;
        assert_eq!(gap_between(Some(Heading), Subtitle), Gap::Tight);
        assert_eq!(gap_between(Some(Subtitle), Section), Gap::AfterHeading);
        assert_eq!(gap_between(Some(Subtitle), Field), Gap::AfterHeading);
        assert!(!row_in_card(Subtitle));
    }
}
