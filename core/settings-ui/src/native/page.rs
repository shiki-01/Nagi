//! 右のスクロールパネル: ページの組み立て・配置・スクロール。

use super::controls::{
    clear_accessible_name, create_child_hidden, get_text, send, send_text, set_accessible_name,
    set_dark_theme, set_font,
};
use super::dpi::Fonts;
use super::pages::{self, RowDef, text_row};
use super::state::{Ctl, Ident, Model, ResultInfo, Role, RowFlags, State};
use super::theme;
use super::widgets::{BtnKind, install_button, install_static};
use crate::native_diag::BadgeKind;
use crate::native_layout::{
    self as layout, BUTTON_W, Columns, RowSpec, button_row, clamp_scroll, control_height,
    layout_rows, scale, scroll_into_view,
};
use crate::native_model::{FieldView, Page, ids};
use crate::native_theme::{
    ButtonStyle, ThemeKind, button_style_for_id, toggle_label_offset, toggle_row_min_height,
};
use nagi_protocol::settings::SettingKind;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    DT_CALCRECT, DT_NOPREFIX, DT_WORDBREAK, DrawTextW, GetDC, HDC, HGDIOBJ, RDW_ALLCHILDREN,
    RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow, ReleaseDC, SelectObject,
};
use windows::Win32::System::SystemServices::{
    SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_LEFT, SS_NOPREFIX, SS_OWNERDRAW,
};
use windows::Win32::UI::Controls::{EM_SETCUEBANNER, SetScrollInfo};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus};
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_SETCHECK, BS_AUTOCHECKBOX, BS_MULTILINE, BS_NOTIFY, BS_PUSHBUTTON, BeginDeferWindowPos,
    CB_ADDSTRING, CB_SETCURSEL, CBS_DROPDOWNLIST, DeferWindowPos, DestroyWindow, ES_AUTOHSCROLL,
    EndDeferWindowPos, GetClientRect, GetParent, SB_VERT, SCROLLINFO, SIF_DISABLENOSCROLL,
    SIF_PAGE, SIF_POS, SIF_RANGE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOZORDER, ShowWindow,
    WM_SETREDRAW, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VSCROLL,
};

/// ロールごとのフォント
pub(super) fn font_for(fonts: Fonts, role: Role) -> windows::Win32::Graphics::Gdi::HFONT {
    match role {
        Role::Heading => fonts.heading,
        Role::Section | Role::Value => fonts.section,
        Role::Label => fonts.label,
        _ => fonts.normal,
    }
}

/// いまのページの見出しの文字列
fn page_title(model: &Model) -> String {
    match model.page {
        Page::Category(ci) => model
            .view
            .as_ref()
            .and_then(|v| v.categories.get(ci))
            .map_or_else(|| "設定".to_owned(), |c| c.name.clone()),
        Page::Status => "設定".to_owned(),
        page => pages::find_page(page).map_or_else(String::new, |p| p.title.to_owned()),
    }
}

/// いまのページの見出しの下の 1 行の説明（無ければ空）。固定ページは定義表の文言、設定カテゴリは項目数
fn page_subtitle(model: &Model) -> String {
    match model.page {
        Page::Category(ci) => model
            .view
            .as_ref()
            .and_then(|v| v.categories.get(ci))
            .map_or_else(String::new, |c| {
                crate::native_model::category_subtitle(
                    c.visible_fields(model.detail).len(),
                    model.detail,
                )
            }),
        Page::Status => String::new(),
        page => pages::find_page(page).map_or_else(String::new, |p| p.subtitle.to_owned()),
    }
}

/// いまのページの行を作る（先頭はページの見出し）
fn rows_for(model: &Model) -> Vec<RowDef> {
    let mut rows = vec![text_row(Role::Heading, page_title(model))];
    let subtitle = page_subtitle(model);
    if !subtitle.is_empty() {
        rows.push(text_row(Role::Subtitle, subtitle));
    }
    match model.page {
        Page::Category(ci) => {
            if let Some(view) = &model.view {
                if let Some(error) = &view.error {
                    rows.push(text_row(
                        Role::Error,
                        format!("設定ファイルを読み込めません: {error}"),
                    ));
                }
                if let Some(category) = view.categories.get(ci) {
                    for fi in category.visible_fields(model.detail) {
                        let field = category.fields[fi].clone();
                        let error = model.errors.get(&field.key).cloned();
                        rows.push(RowDef::Field {
                            field: Box::new(field),
                            error,
                        });
                    }
                }
            }
        }
        Page::Status => rows.extend(pages::status_rows(model)),
        page => {
            if let Some(def) = pages::find_page(page) {
                rows.extend((def.build)(model));
            }
        }
    }
    rows
}

unsafe extern "system" fn combo_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{
        CB_GETDROPPEDSTATE, GetParent, SendMessageW, WM_MOUSEWHEEL,
    };
    if msg == WM_MOUSEWHEEL && send(hwnd, CB_GETDROPPEDSTATE, 0, 0) == 0 {
        // 閉じているコンボボックスはホイールで値が変わってしまうので、パネルのスクロールに回す
        if let Ok(parent) = unsafe { GetParent(hwnd) } {
            unsafe { SendMessageW(parent, msg, Some(wparam), Some(lparam)) };
        }
        return windows::Win32::Foundation::LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

struct Builder<'a> {
    panel: HWND,
    fonts: Fonts,
    ctls: &'a mut Vec<Ctl>,
    next_id: i32,
    row: usize,
    /// ダークテーマか（入力欄・選択欄のテーマに使う）
    dark: bool,
}

impl Builder<'_> {
    fn add(
        &mut self,
        role: Role,
        hwnd: Option<HWND>,
        id: i32,
        key: Option<&str>,
        text: &str,
        width_dp: i32,
    ) -> Option<HWND> {
        let hwnd = hwnd?;
        self.ctls.push(Ctl {
            hwnd,
            id,
            role,
            row: self.row,
            key: key.map(str::to_owned),
            text: text.to_owned(),
            width_dp,
            result: None,
            content_y: 0,
            content_h: 0,
        });
        Some(hwnd)
    }

    fn fresh_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn text(&mut self, role: Role, text: &str, key: Option<&str>) {
        let font = font_for(self.fonts, role);
        let mut style = SS_LEFT.0 | SS_NOPREFIX.0;
        if role == Role::Hint {
            style |= SS_CENTERIMAGE.0 | SS_ENDELLIPSIS.0;
        }
        let id = self.fresh_id();
        let hwnd = create_child_hidden(self.panel, "STATIC", text, style, 0, id, font);
        if let Some(hwnd) = hwnd {
            // 背景と文字をバッファにまとめて描く（ちらつき防止）
            install_static(hwnd, false);
        }
        self.add(role, hwnd, id, key, text, 0);
    }

    /// 節の見出し（直下のカードの見出し）
    fn section(&mut self, text: &str) {
        self.text(Role::Section, text, None);
    }

    /// 診断項目の結果行（オーナードロー。文字は読み上げの名前として `[状態] 項目名` を持つ）
    fn result(&mut self, badge: BadgeKind, label: &str, title: &str) {
        let heading = format!("[{label}] {title}");
        let id = self.fresh_id();
        let hwnd = create_child_hidden(
            self.panel,
            "STATIC",
            &heading,
            SS_OWNERDRAW.0 | SS_NOPREFIX.0,
            0,
            id,
            self.fonts.normal,
        );
        if let Some(hwnd) = hwnd {
            install_static(hwnd, true);
        }
        if self
            .add(Role::Result, hwnd, id, None, &heading, 0)
            .is_some()
            && let Some(c) = self.ctls.last_mut()
        {
            c.result = Some(ResultInfo {
                badge,
                label: label.to_owned(),
                title: title.to_owned(),
            });
        }
    }

    fn button(
        &mut self,
        role: Role,
        text: &str,
        id: i32,
        key: Option<&str>,
        width_dp: i32,
    ) -> Option<HWND> {
        let hwnd = create_child_hidden(
            self.panel,
            "BUTTON",
            text,
            BS_PUSHBUTTON as u32 | BS_NOTIFY as u32 | WS_TABSTOP.0,
            0,
            id,
            self.fonts.normal,
        );
        if let Some(hwnd) = hwnd {
            // 「既定に戻す」は控えめに、主要な操作・取り消せない操作は目立たせる（固定 ID で決まる）
            let style = if role == Role::Reset {
                ButtonStyle::Subtle
            } else {
                button_style_for_id(id)
            };
            install_button(hwnd, BtnKind::Push(style), true);
        }
        self.add(role, hwnd, id, key, text, width_dp)
    }

    fn field(&mut self, field: &FieldView, error: Option<&str>) -> RowFlags {
        let key = field.key.as_str();
        let mut flags = RowFlags::default();
        // 作る順 = Tab 順 = 画面の読み上げ順（入力欄の直前の STATIC がラベルとして読まれる）
        if matches!(field.kind, SettingKind::Bool) {
            let id = self.fresh_id();
            let title = field.title();
            let hwnd = create_child_hidden(
                self.panel,
                "BUTTON",
                &title,
                BS_AUTOCHECKBOX as u32 | BS_MULTILINE as u32 | BS_NOTIFY as u32 | WS_TABSTOP.0,
                0,
                id,
                self.fonts.label,
            );
            if let Some(hwnd) = hwnd {
                // 標準のチェックボックスのまま、見た目だけトグルスイッチにする（役割・名前・状態は変わらない）
                install_button(hwnd, BtnKind::Toggle, true);
            }
            if let Some(hwnd) = self.add(Role::Check, hwnd, id, Some(key), &title, 0)
                && field.bool_value()
            {
                send(hwnd, BM_SETCHECK, 1, 0);
            }
        } else {
            self.text(Role::Label, &field.title(), Some(key));
            flags.has_input = true;
            let id = self.fresh_id();
            match &field.kind {
                SettingKind::Choice(_) => {
                    let hwnd = create_child_hidden(
                        self.panel,
                        "COMBOBOX",
                        "",
                        CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | WS_TABSTOP.0,
                        0,
                        id,
                        self.fonts.normal,
                    );
                    if let Some(hwnd) = self.add(Role::Combo, hwnd, id, Some(key), "", 0) {
                        for entry in field.choice_entries() {
                            send_text(hwnd, CB_ADDSTRING, 0, &entry.label);
                        }
                        let index = field.choice_index().map_or(-1, |i| i as isize);
                        send(hwnd, CB_SETCURSEL, index as usize, 0);
                        set_accessible_name(hwnd, &field.label);
                        set_dark_theme(hwnd, self.dark, false);
                        let _ = unsafe { SetWindowSubclass(hwnd, Some(combo_subclass), 1, 0) };
                    }
                }
                _ => {
                    let value = field.display_text();
                    let hwnd = create_child_hidden(
                        self.panel,
                        "EDIT",
                        &value,
                        ES_AUTOHSCROLL as u32 | WS_TABSTOP.0,
                        WS_EX_CLIENTEDGE.0,
                        id,
                        self.fonts.normal,
                    );
                    if let Some(hwnd) = self.add(Role::Edit, hwnd, id, Some(key), &value, 0)
                        && field.value.is_none()
                    {
                        send_text(hwnd, EM_SETCUEBANNER, 1, "自動");
                    }
                    if let Some(hwnd) = hwnd {
                        set_accessible_name(hwnd, &field.label);
                        set_dark_theme(hwnd, self.dark, false);
                    }
                }
            }
        }
        if !field.description.is_empty() {
            self.text(Role::Desc, &field.description, Some(key));
        }
        if let Some(hint) = field.range_hint() {
            self.text(Role::Hint, &hint, Some(key));
        }
        if field.explicit {
            let id = self.fresh_id();
            // 読み上げの名前に項目名を含める（「既定に戻す」だけでは、どの項目か区別できない）
            if let Some(hwnd) = self.button(Role::Reset, "既定に戻す", id, Some(key), 0) {
                set_accessible_name(hwnd, &format!("{}を既定に戻す", field.label));
            }
        }
        if let Some(error) = error {
            self.text(Role::Error, error, Some(key));
        }
        flags
    }

    fn row(&mut self, def: &RowDef) -> RowFlags {
        match def {
            RowDef::Field { field, error } => self.field(field, error.as_deref()),
            RowDef::Text { role, text } => {
                if *role == Role::Section {
                    self.section(text);
                } else {
                    self.text(*role, text, None);
                }
                RowFlags::default()
            }
            RowDef::Result {
                badge,
                label,
                title,
            } => {
                self.result(*badge, label, title);
                RowFlags::default()
            }
            RowDef::Buttons(buttons) => {
                for b in buttons {
                    let Some(hwnd) = self.button(Role::Button, b.text, b.id, None, BUTTON_W) else {
                        continue;
                    };
                    if let Some(name) = &b.accessible_name {
                        set_accessible_name(hwnd, name);
                    }
                    if !b.enabled {
                        let _ = unsafe { EnableWindow(hwnd, false) };
                    }
                }
                RowFlags::default()
            }
        }
    }
}

/// フォーカスのあるコントロールの目印
fn focused_ident(st: &State) -> Option<Ident> {
    let focus = unsafe { GetFocus() };
    if focus.0.is_null() {
        return None;
    }
    let ui = st.ui.borrow();
    ui.ctls.iter().find(|c| c.hwnd == focus).map(|c| Ident {
        role: c.role,
        key: c.key.clone(),
        id: if c.role == Role::Button { c.id } else { 0 },
    })
}

/// ページを作り直す（コントロールをすべて作り直して配置する）。スクロール位置は保つ
pub(super) fn rebuild_page(st: &State) {
    let panel = st.panel.get();
    let Some(fonts) = st.fonts.get() else {
        return;
    };
    if panel.0.is_null() {
        return;
    }
    let focus = focused_ident(st);
    // 何にもフォーカスが無いとき（押したボタンが無効化されて外れた等）だけ、前の目印を持ち越す。
    // 別の場所（左の一覧など）にフォーカスがあるなら、そこから奪わない
    let current = unsafe { GetFocus() };
    let nothing_focused =
        current.0.is_null() || current == panel || Ok(current) == unsafe { GetParent(panel) };
    let defs = {
        let mut model = st.model.borrow_mut();
        if focus.is_some() {
            model.wanted_focus = focus;
        } else if !nothing_focused {
            model.wanted_focus = None;
        }
        rows_for(&model)
    };
    // 作り直している間は再描画を止める（壊してから作る途中の姿を見せない）。終わったら一度に描く
    // 新しい部品は非表示で作り、フォント・位置・サイズが決まってから出す。古い部品は、新しいものを出したあとで壊す
    let started = std::time::Instant::now();
    send(panel, WM_SETREDRAW, 0, 0);
    let old = std::mem::take(&mut st.ui.borrow_mut().ctls);
    let mut ctls = Vec::new();
    let mut flags = Vec::new();
    {
        let mut builder = Builder {
            panel,
            fonts,
            ctls: &mut ctls,
            next_id: ids::DYNAMIC_BASE,
            row: 0,
            dark: theme::palette().kind == ThemeKind::Dark,
        };
        for (row, def) in defs.iter().enumerate() {
            builder.row = row;
            let mut f = builder.row(def);
            f.gap = pages::gap_before(row.checked_sub(1).map(|p| &defs[p]), def);
            f.in_card = pages::in_card(def);
            f.style = pages::style_of(row, def);
            flags.push(f);
        }
    }
    {
        let mut ui = st.ui.borrow_mut();
        ui.ctls = ctls;
        ui.flags = flags;
        ui.specs = vec![RowSpec::default(); defs.len()];
    }
    let count = ctls_len(st);
    relayout(st);
    // 位置・サイズが確定してから、ページ単位で一度だけ表示する
    for c in &st.ui.borrow().ctls {
        let _ = unsafe { ShowWindow(c.hwnd, SW_SHOWNA) };
    }
    for c in old {
        if matches!(
            c.role,
            Role::Edit | Role::Combo | Role::Reset | Role::Button
        ) {
            clear_accessible_name(c.hwnd);
        }
        let _ = unsafe { DestroyWindow(c.hwnd) };
    }
    // フォーカスを戻す前に再描画を戻す（見えていないウィンドウにはフォーカスを移せない）
    send(panel, WM_SETREDRAW, 1, 0);
    redraw_panel(panel);
    restore_focus(st);
    log_rebuild(count, started.elapsed());
}

fn ctls_len(st: &State) -> usize {
    st.ui.borrow().ctls.len()
}

/// ページの再構築にかかった時間（デバッグ用。環境変数 `NAGI_SETTINGS_DEBUG` があるときだけ stderr へ。入力内容は含めない）
fn log_rebuild(count: usize, elapsed: std::time::Duration) {
    if std::env::var_os("NAGI_SETTINGS_DEBUG").is_some() {
        eprintln!(
            "[debug] settings page rebuilt: {count} controls, {:.1} ms",
            elapsed.as_secs_f64() * 1000.0
        );
    }
}

/// パネルとその子を、その場で一度に描き直す
fn redraw_panel(panel: HWND) {
    let _ = unsafe {
        RedrawWindow(
            Some(panel),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
        )
    };
}

fn restore_focus(st: &State) {
    let wanted = st.model.borrow().wanted_focus.clone();
    let Some(wanted) = wanted else { return };
    let target = {
        let ui = st.ui.borrow();
        ui.ctls
            .iter()
            .find(|c| {
                c.role == wanted.role
                    && c.key == wanted.key
                    && (c.role != Role::Button || c.id == wanted.id)
            })
            .map(|c| c.hwnd)
    };
    if let Some(hwnd) = target {
        let enabled = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(hwnd) };
        if enabled.as_bool() {
            let _ = unsafe { SetFocus(Some(hwnd)) };
            st.model.borrow_mut().wanted_focus = None;
        }
    }
}

/// 文字列の折り返し後の高さ（px）
fn measure(hdc: HDC, font: windows::Win32::Graphics::Gdi::HFONT, text: &str, width: i32) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let old = unsafe { SelectObject(hdc, HGDIOBJ(font.0)) };
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width.max(1),
        bottom: 0,
    };
    unsafe {
        DrawTextW(
            hdc,
            &mut buf,
            &mut rect,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
        )
    };
    unsafe { SelectObject(hdc, old) };
    rect.bottom - rect.top
}

fn panel_client(panel: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(panel, &mut rect) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// 折り返しの高さを測り直し、配置を計算して反映する（サイズ・DPI・内容が変わったとき）
pub(super) fn relayout(st: &State) {
    let panel = st.panel.get();
    let Some(fonts) = st.fonts.get() else {
        return;
    };
    if panel.0.is_null() {
        return;
    }
    let dpi = st.dpi.get();
    let (width, height) = panel_client(panel);
    let cols = layout::columns(width, dpi);
    let control_h = control_height(dpi);
    let pad = scale(2, dpi);
    // 見出し・節の見出しはカードの外（カードの左端に揃え、カードの幅で折り返す）
    let outer_w = cols.card_w;
    let section_w = cols.section_w;
    let hdc = unsafe { GetDC(Some(panel)) };
    {
        let mut ui = st.ui.borrow_mut();
        let mut specs = vec![RowSpec::default(); ui.specs.len()];
        for (spec, flag) in specs.iter_mut().zip(&ui.flags) {
            spec.gap = flag.gap;
            spec.in_card = flag.in_card;
            spec.style = flag.style;
        }
        for c in &ui.ctls {
            let flag = ui.flags[c.row];
            let spec = &mut specs[c.row];
            match c.role {
                Role::Heading => spec.title_h = measure(hdc, fonts.heading, &c.text, outer_w) + pad,
                Role::Subtitle => spec.title_h = measure(hdc, fonts.normal, &c.text, outer_w) + pad,
                Role::Section => {
                    spec.title_h = measure(hdc, fonts.section, &c.text, section_w) + pad
                }
                Role::Value => {
                    spec.title_h = measure(hdc, fonts.section, &c.text, cols.width) + pad
                }
                Role::Result => spec.title_h = control_h,
                Role::Title => spec.title_h = measure(hdc, fonts.normal, &c.text, cols.width) + pad,
                Role::Label => spec.title_h = measure(hdc, fonts.label, &c.text, cols.width) + pad,
                Role::Check => {
                    let h = measure(
                        hdc,
                        fonts.label,
                        &c.text,
                        cols.check_w - toggle_label_offset(dpi),
                    );
                    spec.title_h = spec.title_h.max((h + pad).max(toggle_row_min_height(dpi)));
                }
                Role::Desc => spec.desc_h = measure(hdc, fonts.normal, &c.text, cols.width) + pad,
                Role::Error => spec.error_h = measure(hdc, fonts.normal, &c.text, cols.width) + pad,
                Role::Reset if !flag.has_input => {
                    // 真偽値の項目は、チェックボックスと同じ行の右端に置く
                    spec.title_h = spec.title_h.max(control_h);
                }
                Role::Edit | Role::Combo | Role::Reset | Role::Button => spec.control_h = control_h,
                Role::Hint => {}
            }
        }
        ui.layout = Some(layout_rows(&specs, dpi));
        ui.specs = specs;
        ui.viewport_h = height;
        let content = ui.layout.as_ref().map_or(0, |l| l.content_height);
        ui.scroll = clamp_scroll(ui.scroll, content, height);
    }
    if !hdc.is_invalid() {
        unsafe { ReleaseDC(Some(panel), hdc) };
    }
    place_all(st);
}

/// 計算済みの配置をコントロールに反映する（スクロールのたびに呼ぶ）
fn place_all(st: &State) {
    let panel = st.panel.get();
    let dpi = st.dpi.get();
    let (width, _) = panel_client(panel);
    let cols: Columns = layout::columns(width, dpi);
    let control_h = control_height(dpi);
    let mut moves: Vec<(HWND, i32, i32, i32, i32)> = Vec::new();
    let (content, viewport, scroll);
    {
        let mut ui = st.ui.borrow_mut();
        let Some(panel_layout) = ui.layout.clone() else {
            return;
        };
        scroll = ui.scroll;
        content = panel_layout.content_height;
        viewport = ui.viewport_h;
        let flags = ui.flags.clone();
        let specs = ui.specs.clone();
        // ボタンの行ごとの位置
        let mut button_slots: Vec<Vec<(i32, i32)>> = vec![Vec::new(); specs.len()];
        for (row, slots) in button_slots.iter_mut().enumerate() {
            let widths: Vec<i32> = ui
                .ctls
                .iter()
                .filter(|c| c.row == row && c.role == Role::Button)
                .map(|c| c.width_dp)
                .collect();
            if !widths.is_empty() {
                *slots = button_row(cols.x, &widths, dpi);
            }
        }
        let mut button_seen = vec![0usize; specs.len()];
        let drop_h = scale(240, dpi);
        for c in &mut ui.ctls {
            let rl = panel_layout.rows[c.row];
            let spec = specs[c.row];
            let flag = flags[c.row];
            let (x, y, w, h, content_h) = match c.role {
                Role::Heading | Role::Subtitle => (
                    cols.card_x,
                    rl.title_y,
                    cols.card_w,
                    spec.title_h,
                    spec.title_h,
                ),
                // 左に印を付けるぶん、カードの左端より内側から始める
                Role::Section => (
                    cols.section_x,
                    rl.title_y,
                    cols.section_w,
                    spec.title_h,
                    spec.title_h,
                ),
                Role::Title | Role::Label | Role::Value | Role::Result => {
                    (cols.x, rl.title_y, cols.width, spec.title_h, spec.title_h)
                }
                Role::Check => (cols.x, rl.title_y, cols.check_w, spec.title_h, spec.title_h),
                Role::Desc => (cols.x, rl.desc_y, cols.width, spec.desc_h, spec.desc_h),
                Role::Error => (cols.x, rl.error_y, cols.width, spec.error_h, spec.error_h),
                Role::Edit => (cols.x, rl.control_y, cols.control_w, control_h, control_h),
                Role::Combo => (
                    cols.x,
                    rl.control_y,
                    cols.control_w,
                    control_h + drop_h,
                    control_h,
                ),
                Role::Hint => (cols.hint_x, rl.control_y, cols.hint_w, control_h, control_h),
                Role::Reset if !flag.has_input => {
                    (cols.reset_x, rl.title_y, cols.reset_w, control_h, control_h)
                }
                // 入力欄の行では、案内の有無によらず同じ右端の列に揃える
                Role::Reset => (
                    cols.reset_x,
                    rl.control_y,
                    cols.reset_w,
                    control_h,
                    control_h,
                ),
                Role::Button => {
                    let n = button_seen[c.row];
                    button_seen[c.row] += 1;
                    let (x, w) = button_slots[c.row].get(n).copied().unwrap_or((cols.x, 0));
                    (x, rl.control_y, w, control_h, control_h)
                }
            };
            c.content_y = y;
            c.content_h = content_h;
            moves.push((c.hwnd, x, y - scroll, w.max(0), h.max(0)));
        }
    }
    unsafe {
        if let Ok(mut hdwp) = BeginDeferWindowPos(moves.len() as i32) {
            for (hwnd, x, y, w, h) in &moves {
                match DeferWindowPos(
                    hdwp,
                    *hwnd,
                    None,
                    *x,
                    *y,
                    *w,
                    *h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                ) {
                    Ok(next) => hdwp = next,
                    Err(_) => break,
                }
            }
            let _ = EndDeferWindowPos(hdwp);
        }
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
            nMin: 0,
            nMax: (content - 1).max(0),
            nPage: viewport.max(0) as u32,
            nPos: scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(panel, SB_VERT, &info, true);
    }
    // 背景（カード）と子の文字を、同じ回でまとめて描く（別々のタイミングで描くとちらつく）
    redraw_panel(panel);
}

/// スクロール位置を設定する（範囲内に丸める）
pub(super) fn scroll_to(st: &State, pos: i32) {
    {
        let mut ui = st.ui.borrow_mut();
        let content = ui.layout.as_ref().map_or(0, |l| l.content_height);
        let clamped = clamp_scroll(pos, content, ui.viewport_h);
        if clamped == ui.scroll {
            return;
        }
        ui.scroll = clamped;
    }
    place_all(st);
}

/// 現在のスクロール位置・最大値・ページ高さ
pub(super) fn scroll_state(st: &State) -> (i32, i32, i32) {
    let ui = st.ui.borrow();
    let content = ui.layout.as_ref().map_or(0, |l| l.content_height);
    (
        ui.scroll,
        layout::max_scroll(content, ui.viewport_h),
        ui.viewport_h,
    )
}

/// ID のコントロールが見える位置までスクロールする（Tab で画面外のコントロールに移ったとき）
pub(super) fn ensure_visible(st: &State, id: i32) {
    let target = {
        let ui = st.ui.borrow();
        let content = ui.layout.as_ref().map_or(0, |l| l.content_height);
        ui.ctls.iter().find(|c| c.id == id).map(|c| {
            let margin = scale(8, st.dpi.get());
            scroll_into_view(
                ui.scroll,
                ui.viewport_h,
                c.content_y,
                c.content_y + c.content_h,
                content,
                margin,
            )
        })
    };
    if let Some(pos) = target {
        scroll_to(st, pos);
    }
}

/// ID からコントロールの情報を引く（役割・キー・ウィンドウ）
pub(super) fn find_ctl(st: &State, id: i32) -> Option<(Role, Option<String>, HWND)> {
    let ui = st.ui.borrow();
    ui.ctls
        .iter()
        .find(|c| c.id == id)
        .map(|c| (c.role, c.key.clone(), c.hwnd))
}

/// ウィンドウ ハンドルからコントロールの役割を引く（`WM_CTLCOLORSTATIC` 用）
pub(super) fn role_of(st: &State, hwnd: HWND) -> Option<Role> {
    let ui = st.ui.borrow();
    ui.ctls.iter().find(|c| c.hwnd == hwnd).map(|c| c.role)
}

/// 入力欄の文字列を読む
pub(super) fn edit_text(hwnd: HWND) -> String {
    get_text(hwnd)
}

/// フォントを全コントロールに付け直す（DPI が変わったとき）
pub(super) fn refit_fonts(st: &State) {
    let Some(fonts) = st.fonts.get() else { return };
    let ui = st.ui.borrow();
    for c in &ui.ctls {
        set_font(c.hwnd, font_for(fonts, c.role));
    }
}
