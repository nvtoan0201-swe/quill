//! Window chrome laid out after the original's Library screenshot (ia.net
//! support pages): Organizer │ File List │ Editor, each with a 34 pt top bar.

use super::*;

pub(super) const BAR_H: f32 = 37.;
const ORGANIZER_W: f32 = 172.;
const LIST_W: f32 = 240.;

fn small_icon(id: &str, icon: &str, t: &Theme, enabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .size(px(24.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| d.cursor(CursorStyle::PointingHand).hover(|s| s.bg(t.hover)))
        .child(
            svg()
                .path(SharedString::from(format!("icons/{icon}.svg")))
                .size(px(16.))
                .text_color(if enabled { t.toolbar_icon } else { t.tertiary }),
        )
}

fn bar_drag(d: gpui::Div) -> gpui::Div {
    d.on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _| {
        if ev.click_count == 2 {
            log::debug!("title bar double click: zoom");
            window.zoom_window()
        } else {
            log::debug!("title bar drag: start_window_move");
            window.start_window_move()
        }
    })
}

impl Workspace {
    // ---- navigation history -------------------------------------------------

    pub(super) fn doc_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.doc_back.pop() else { return };
        if let Some(cur) = self.editor().read(cx).path.clone() {
            self.doc_fwd.push(cur);
        }
        self.navigating = true;
        self.open_path(&p, true, window, cx);
        self.navigating = false;
    }

    pub(super) fn doc_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.doc_fwd.pop() else { return };
        if let Some(cur) = self.editor().read(cx).path.clone() {
            self.doc_back.push(cur);
        }
        self.navigating = true;
        self.open_path(&p, true, window, cx);
        self.navigating = false;
    }

    fn dir_back(&mut self, cx: &mut Context<Self>) {
        if self.smart.is_some() || self.tag.is_some() {
            let d = self.lib_dir.clone();
            return self.enter_folder(d, cx);
        }
        let Some(d) = self.dir_back.pop() else { return };
        self.dir_fwd.push(self.lib_dir.clone());
        self.lib_dir = d;
        self.refresh_library();
        cx.notify();
    }

    fn dir_forward(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dir_fwd.pop() else { return };
        self.dir_back.push(self.lib_dir.clone());
        self.lib_dir = d;
        self.refresh_library();
        cx.notify();
    }

    fn show_smart(&mut self, query: String, cx: &mut Context<Self>) {
        self.tag = None;
        self.smart = Some(query);
        self.refresh_library();
        cx.notify();
    }

    pub(super) fn toggle_favorite(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| {
            if let Some(i) = s.favorites.iter().position(|p| *p == path) {
                s.favorites.remove(i);
            } else {
                s.favorites.push(path);
            }
        });
    }

    // ---- organizer ------------------------------------------------------------

    pub(super) fn render_organizer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *cx.global::<Theme>();
        let section = |label: &str| {
            let first = label == "Locations";
            div()
                .mt(px(if first { 3. } else { 9. }))
                .mb(px(2.))
                .px(px(14.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.section_header)
                .child(label.to_string())
        };
        let row = |id: String, icon: AnyElement, label: String, selected: bool| {
            div()
                .id(SharedString::from(id))
                .mx(px(8.))
                .px(px(9.))
                .h(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.5))
                .text_color(t.organizer_text)
                .when(selected, |d| d.bg(gpui::rgba(0x0000001a)))
                .cursor(CursorStyle::PointingHand)
                .child(icon)
                .child(div().truncate().child(label))
        };
        let folder_icon = |c: Hsla| svg().path("icons/folder.svg").size(px(16.)).text_color(c).into_any_element();
        let plain = self.tag.is_none() && self.smart.is_none();

        let mut col = div()
            .id("organizer")
            .w(px(ORGANIZER_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.organizer_bg)
            .rounded_tl(self.corner)
            .rounded_bl(self.corner)
            .border_r_1()
            .border_color(t.divider)
            .font_family(crate::fonts::ui())
            .child(
                bar_drag(div().h(px(BAR_H)).flex_none().flex().items_center())
                    .child(traffic_lights(window, &t))
                    .child(
                        div()
                            .ml(px(18.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(small_icon("org-sidebar", "sidebar", &t, true).on_click(cx.listener(|ws, _, _, cx| {
                                ws.update_settings(cx, |s| s.show_library = !s.show_library)
                            }))),
                    ),
            );

        let mut body = div().id("organizer-body").flex_1().overflow_y_scroll().flex().flex_col();
        body = body.child(section("Locations")).child(
            row("org-lib".into(), folder_icon(t.system_folder), "Library".into(), plain && self.lib_dir == self.lib_root)
                .on_click(cx.listener(|ws, _, _, cx| {
                    let r = ws.lib_root.clone();
                    ws.enter_folder(r, cx)
                })),
        );
        let mut folders: Vec<PathBuf> = std::fs::read_dir(&self.lib_root)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !library::file_stem(p).starts_with('.')).collect())
            .unwrap_or_default();
        folders.sort();
        for (i, f) in folders.into_iter().enumerate() {
            let sel = plain && self.lib_dir == f;
            body = body.child(
                row(format!("org-f{i}"), folder_icon(t.system_folder), library::file_stem(&f), sel)
                    .pl(px(25.))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.enter_folder(f.clone(), cx))),
            );
        }

        body = body.child(section("Favorites"));
        for (i, fav) in self.settings.favorites.clone().into_iter().enumerate() {
            let is_dir = fav.is_dir();
            let icon = if is_dir {
                folder_icon(t.system_folder)
            } else {
                svg().path("icons/doc.svg").size(px(16.)).text_color(t.toolbar_icon).into_any_element()
            };
            let (open, unpin) = (fav.clone(), fav.clone());
            body = body.child(
                row(format!("org-fav{i}"), icon, library::file_stem(&fav), plain && self.lib_dir == fav)
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        if open.is_dir() {
                            ws.enter_folder(open.clone(), cx)
                        } else {
                            ws.open_path(&open, true, window, cx)
                        }
                    }))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |ws, ev: &MouseDownEvent, window, cx| {
                        let p = unpin.clone();
                        let weak = cx.entity().downgrade();
                        let items = vec![call("Remove from Favorites", move |_, cx| {
                            let _ = weak.update(cx, |ws, cx| ws.toggle_favorite(p.clone(), cx));
                        })];
                        ws.open_menu(items, ev.position, window, cx);
                    })),
            );
        }

        body = body.child(section("Smart Folders"));
        for (i, q) in self.settings.smart_folders.clone().into_iter().enumerate() {
            let sel = self.smart.as_deref() == Some(q.as_str());
            let (open, remove) = (q.clone(), q.clone());
            body = body.child(
                row(format!("org-smart{i}"), svg().path("icons/search.svg").size(px(15.)).text_color(t.toolbar_icon).into_any_element(), q, sel)
                    .on_click(cx.listener(move |ws, _, _, cx| ws.show_smart(open.clone(), cx)))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |ws, ev: &MouseDownEvent, window, cx| {
                        let q = remove.clone();
                        let weak = cx.entity().downgrade();
                        let items = vec![call("Delete Smart Folder", move |_, cx| {
                            let q = q.clone();
                            let _ = weak.update(cx, |ws, cx| ws.update_settings(cx, |s| s.smart_folders.retain(|x| *x != q)));
                        })];
                        ws.open_menu(items, ev.position, window, cx);
                    })),
            );
        }

        body = body.child(section("Hashtags"));
        for (i, (tag, _)) in self.tags.clone().into_iter().enumerate() {
            let sel = self.tag.as_deref() == Some(tag.as_str());
            let tag2 = tag.clone();
            let hash = div().w(px(16.)).flex().justify_center().text_color(t.folder).font_weight(FontWeight::BOLD).child("#").into_any_element();
            body = body.child(row(format!("org-tag{i}"), hash, tag, sel).on_click(cx.listener(move |ws, _, _, cx| {
                ws.smart = None;
                ws.tag = Some(tag2.clone());
                ws.refresh_library();
                cx.notify();
            })));
        }
        col = col.child(body);
        col
    }

    // ---- file list ------------------------------------------------------------

    pub(super) fn render_file_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *cx.global::<Theme>();
        let active_path = self.editor().read(cx).path.clone();
        let filter = self.filter_text.to_lowercase();
        let entries: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| filter.is_empty() || e.title.to_lowercase().contains(&filter) || e.excerpt.to_lowercase().contains(&filter))
            .cloned()
            .collect();
        let (title, icon) = match (&self.smart, &self.tag) {
            (Some(q), _) => (q.clone(), "search"),
            (_, Some(tag)) => (format!("#{tag}"), "folder"),
            _ if self.lib_dir == self.lib_root => ("Library".to_string(), "folder"),
            _ => (library::file_stem(&self.lib_dir), "folder"),
        };
        let can_back = !self.dir_back.is_empty() || self.smart.is_some() || self.tag.is_some();
        let can_fwd = !self.dir_fwd.is_empty();
        let now = SystemTime::now();
        let offset = library::utc_offset();
        let lights_here = !self.settings.show_organizer;

        let header = bar_drag(
            div()
                .h(px(BAR_H))
                .flex_none()
                .flex()
                .items_center()
                .relative()
                .border_b_1()
                .border_color(t.toolbar_border),
        )
        .when(lights_here, |d| d.child(traffic_lights(window, &t)))
        .child(
            div()
                .flex()
                .gap(px(4.))
                .pl(px(if lights_here { 4. } else { 12. }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(small_icon("list-back", "chevron-left", &t, can_back).on_click(cx.listener(|ws, _, _, cx| ws.dir_back(cx))))
                .child(small_icon("list-fwd", "chevron-right", &t, can_fwd).on_click(cx.listener(|ws, _, _, cx| ws.dir_forward(cx)))),
        )
        .child(
            div()
                .absolute()
                .left(px(80.))
                .right(px(16.))
                .flex()
                .justify_center()
                .items_center()
                .gap(px(6.))
                .child(
                    svg()
                        .path(SharedString::from(format!("icons/{icon}.svg")))
                        .size(px(16.))
                        .text_color(if icon == "folder" { t.system_folder } else { t.toolbar_icon }),
                )
                .child(div().text_size(px(15.)).text_color(t.toolbar_title).truncate().child(title)),
        );

        let sort_label = match self.settings.sort {
            SortBy::Name => "Sort by Name",
            SortBy::Modified => "Sort by Date",
        };
        let sort_bar = div()
            .h(px(22.))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .bg(t.sortbar_bg)
            .child(
                div()
                    .id("sort-menu")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor(CursorStyle::PointingHand)
                    .text_size(px(9.5))
                    .text_color(t.list_meta)
                    .child(sort_label)
                    .child(svg().path("icons/chevron-down.svg").size(px(10.)).text_color(t.list_meta))
                    .on_click(cx.listener(|ws, ev: &gpui::ClickEvent, window, cx| {
                        let s = &ws.settings;
                        let items = vec![
                            check("Name", SortByName, s.sort == SortBy::Name),
                            check("Date Modified", SortByDate, s.sort == SortBy::Modified),
                        ];
                        let p = ev.position();
                        ws.open_menu(items, point(p.x - px(10.), p.y + px(10.)), window, cx);
                    })),
            );

        let mut list = div().id("library-list").flex_1().overflow_y_scroll().flex().flex_col();
        for (i, e) in entries.into_iter().enumerate() {
            let selected = active_path.as_deref() == Some(e.path.as_path());
            let path = e.path.clone();
            let ctx_path = e.path.clone();
            let full = e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // Character budget for the title next to the date (≈0.45 em per glyph at 10 px; Plex Sans runs wider than SF, so it is set smaller).
            let name = crate::markdown::truncate_middle(&full, ((LIST_W - 32. - 12. - 8. - 50.) / (10. * 0.45)) as usize);
            let row = div()
                .id(SharedString::from(format!("lib-{i}")))
                .relative()
                .flex()
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(t.hover))
                .on_click(cx.listener(move |ws, _, window, cx| {
                    if path.is_dir() {
                        ws.enter_folder(path.clone(), cx);
                    } else {
                        ws.open_path(&path, true, window, cx);
                    }
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(move |ws, ev: &MouseDownEvent, window, cx| {
                    let items = ws.library_item_menu(ctx_path.clone(), cx);
                    ws.open_menu(items, ev.position, window, cx);
                }))
                .when(selected, |d| d.child(div().absolute().left_0().top_0().bottom_0().w(px(4.)).bg(t.selection_bar)));
            let row = if e.is_dir {
                row.child(
                    div()
                        .flex_1()
                        .h(px(36.))
                        .pl(px(12.))
                        .pr(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .border_b_1()
                        .border_color(t.list_separator)
                        .child(svg().path("icons/folder.svg").size(px(16.)).text_color(t.system_folder))
                        .child(div().flex_1().text_size(px(13.)).text_color(t.list_title).truncate().child(name))
                        .child(div().text_size(px(12.)).text_color(t.list_meta).child(e.count.to_string()))
                        .child(svg().path("icons/chevron-right.svg").size(px(10.)).text_color(t.list_meta)),
                )
            } else {
                let date = library::list_date(e.modified, now, offset);
                row.child(
                    div()
                        .w(px(32.))
                        .flex_none()
                        .pt(px(7.))
                        .flex()
                        .justify_center()
                        .child(svg().path("icons/doc.svg").size(px(14.)).text_color(t.doc_icon)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pt(px(6.))
                        .pb(px(7.))
                        .pr(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .border_b_1()
                        .border_color(t.list_separator)
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(div().flex_1().min_w_0().text_size(px(10.)).line_height(px(15.)).text_color(t.list_title).truncate().child(name))
                                .child(div().flex_none().text_size(px(10.)).line_height(px(15.)).text_color(t.list_meta).child(date)),
                        )
                        .when(!e.excerpt.is_empty(), |d| {
                            d.child(div().text_size(px(10.)).line_height(px(13.5)).text_color(t.list_meta).line_clamp(2).child(e.excerpt.clone()))
                        }),
                )
            };
            list = list.child(row);
        }

        div()
            .w(px(LIST_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.list_bg)
            .when(lights_here, |d| d.rounded_tl(self.corner).rounded_bl(self.corner))
            .border_r_1()
            .border_color(t.divider)
            .font_family(crate::fonts::ui())
            .child(header)
            .child(sort_bar)
            .when(self.show_filter, |d| {
                let q = self.filter_text.clone();
                d.child(
                    div()
                        .mx(px(10.))
                        .my(px(6.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(t.hover)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(13.))
                        .text_color(t.text)
                        .child(svg().path("icons/search.svg").size(px(13.)).text_color(t.list_meta))
                        .child(div().flex_1().child(self.filter.clone()))
                        .when(!q.trim().is_empty(), |d| {
                            d.child(
                                div()
                                    .id("save-smart")
                                    .text_size(px(11.))
                                    .text_color(t.accent)
                                    .cursor(CursorStyle::PointingHand)
                                    .child("Save")
                                    .on_click(cx.listener(move |ws, _, _, cx| {
                                        let q = q.trim().to_string();
                                        ws.update_settings(cx, |s| {
                                            if !s.smart_folders.contains(&q) {
                                                s.smart_folders.push(q.clone());
                                            }
                                        });
                                        ws.show_toast(format!("Smart Folder “{q}” saved"), cx);
                                    })),
                            )
                        }),
                )
            })
            .child(list)
    }

    // ---- editor toolbar ---------------------------------------------------------

    pub(super) fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *cx.global::<Theme>();
        let e = self.editor().read(cx);
        let name = e
            .path
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| e.title());
        let edited = e.buffer.is_dirty() && e.path.is_none();
        let visible = self.chrome_visible(self.settings.title_bar, cx);
        let show_lib = self.settings.show_library;
        let (can_back, can_fwd) = (!self.doc_back.is_empty(), !self.doc_fwd.is_empty());
        let focus_on = self.settings.focus != FocusMode::Off || self.settings.syntax || self.settings.style_check;

        bar_drag(
            div()
                .h(px(BAR_H))
                .flex_none()
                .flex()
                .items_center()
                .relative()
                .bg(t.toolbar_bg)
                .rounded_tr(self.corner)
                .when(!show_lib, |d| d.rounded_tl(self.corner))
                .border_b_1()
                .border_color(t.toolbar_border)
                .font_family(crate::fonts::ui())
                .opacity(if visible { 1. } else { 0. }),
        )
        .when(!show_lib, |d| {
            d.child(traffic_lights(window, &t)).child(
                div()
                    .ml(px(10.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(small_icon("tb-sidebar", "sidebar", &t, true).on_click(cx.listener(|ws, _, _, cx| {
                        ws.update_settings(cx, |s| s.show_library = true)
                    }))),
            )
        })
        .child(
            div()
                .flex()
                .gap(px(4.))
                .pl(px(12.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(small_icon("doc-back", "chevron-left", &t, can_back).on_click(cx.listener(|ws, _, window, cx| ws.doc_back(window, cx))))
                .child(small_icon("doc-fwd", "chevron-right", &t, can_fwd).on_click(cx.listener(|ws, _, window, cx| ws.doc_forward(window, cx)))),
        )
        .child(
            div()
                .absolute()
                .left(px(160.))
                .right(px(160.))
                .flex()
                .justify_center()
                .items_center()
                .gap(px(6.))
                .child(svg().path("icons/doc.svg").size(px(14.)).text_color(t.doc_icon))
                .child(div().text_size(px(13.)).text_color(t.toolbar_title).truncate().child(name))
                .when(edited, |d| d.child(div().text_size(px(13.)).text_color(t.list_meta).child("— Edited"))),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .pr(px(12.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("tb-focus")
                        .h(px(24.))
                        .px(px(4.))
                        .rounded(px(5.))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(t.hover))
                        .child(svg().path("icons/focus-lines.svg").size(px(18.)).text_color(if focus_on { t.accent } else { t.toolbar_icon }))
                        .child(div().w(px(1.)).h(px(14.)).bg(t.toolbar_border))
                        .child(svg().path("icons/chevron-down.svg").size(px(10.)).text_color(t.toolbar_icon))
                        .on_click(cx.listener(|ws, ev: &gpui::ClickEvent, window, cx| {
                            let items = ws.focus_menu(cx);
                            let p = ev.position();
                            ws.open_menu(items, point(p.x - px(menus::WIDTH - 30.), p.y + px(16.)), window, cx);
                        })),
                )
                .child(
                    small_icon("tb-preview", "play", &t, true)
                        .when(self.settings.preview, |d| d.bg(t.hover))
                        .on_click(cx.listener(|ws, _, _, cx| ws.update_settings(cx, |s| s.preview = !s.preview))),
                ),
        )
        // The original relies on the macOS menu bar; on Linux the menus open from
        // a right click on the toolbar (or F10).
        .on_mouse_down(MouseButton::Right, cx.listener(|ws, ev: &MouseDownEvent, window, cx| {
            let items = ws.main_menu(cx);
            ws.open_menu(items, ev.position, window, cx);
        }))
    }
}
