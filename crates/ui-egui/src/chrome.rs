//! Window chrome: tab strip (with the integrated macOS title bar), mode bar, right rail.

use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, Stroke, vec2};
#[cfg(target_os = "windows")]
use egui::{CursorIcon, Pos2, Vec2, pos2};

use crate::canvas::{DocView, Fit, PageLayout};
use crate::theme::{self, ThemePreference, Tokens};
use crate::{Dialog, Mode, PdfCraftApp, PropsTab, RightPanel, icons, widgets};

pub fn tab_strip(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let left = if cfg!(target_os = "macos") && app.integrated_titlebar { 80 } else { 8 };
    egui::Panel::top("tab_strip")
        .exact_size(38.0)
        .frame(egui::Frame::NONE.fill(t.titlebar).inner_margin(egui::Margin { left, right: 10, top: 0, bottom: 0 }))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let drag = ui.interact(full, ui.id().with("titledrag"), Sense::click_and_drag());
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icons::button(ui, "house", 28.0, app.active.is_none() && !app.combine_showing(), tl!("Home")).clicked() {
                    app.active = None;
                    app.combine_tab.focused = false;
                }
                // Reserve exactly what the fixed controls (Open button, right-side icons)
                // took last frame, so tabs never slide under them at any font scale or
                // window width (first frame estimates).
                let reserve_id = ui.id().with("tab-reserve");
                let fallback = if cfg!(target_os = "windows") { 250.0 } else { 152.0 };
                let reserve = ui.data_mut(|data| data.get_temp::<f32>(reserve_id)).unwrap_or(fallback);
                // Tabs scroll horizontally when more are open than fit (wheel included); the
                // active tab scrolls into view whenever the selection changes.
                let state = (app.active, app.views.len(), app.combine_showing());
                let changed = ui.data_mut(|data| {
                    let id = ui.id().with("active_tab");
                    let changed = data.get_temp::<(Option<usize>, usize, bool)>(id) != Some(state);
                    data.insert_temp(id, state);
                    changed
                });
                let mut close = None;
                ui.scope(|ui| {
                    ui.style_mut().always_scroll_the_only_direction = true;
                    egui::ScrollArea::horizontal()
                        .id_salt("document_tabs")
                        .max_width((ui.available_width() - reserve).max(0.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                for i in 0..app.views.len() {
                                    let Some(doc) = app.session.get(app.views[i].id) else { continue };
                                    let (name, dirty) = (doc.display_name(), doc.dirty);
                                    let response = tab(ui, &t, "file-text", &name, dirty, app.active == Some(i), &mut close, i);
                                    if changed && app.active == Some(i) && !app.combine_showing() {
                                        response.scroll_to_me(Some(Align::Center));
                                    }
                                    if response.clicked() {
                                        app.active = Some(i);
                                    }
                                }
                                if let Some(i) = close {
                                    app.request_close_tab(i);
                                }
                                if app.combine_tab.open {
                                    // After the document tabs; its index can't clash with theirs.
                                    let mut close = None;
                                    let response = tab(ui, &t, "files", tl!("Combine files"), false, app.combine_showing(), &mut close, usize::MAX);
                                    if changed && app.combine_showing() {
                                        response.scroll_to_me(Some(Align::Center));
                                    }
                                    if response.clicked() {
                                        app.open_combine_tab();
                                    }
                                    if close.is_some() {
                                        app.close_combine_tab();
                                    }
                                }
                                ui.add_space(4.0);
                            });
                        });
                });
                // The Open button stays fixed after the tabs (always reachable without
                // scrolling to the very end), then the right-side icons. `before` is
                // measured before either, so the reserved width covers both: measuring
                // after the Open button made the tabs too wide by its width and pushed
                // the rightmost window buttons (Close, Maximize) past the panel edge.
                let before = ui.available_width();
                ui.add_space(4.0);
                if widgets::ghost_button(ui, "plus", tl!("Open")).on_hover_text(tl!("Open a PDF (⌘O)")).clicked() {
                    app.open_dialog();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Windows draws no native title bar: minimize, maximize/restore and close
                    // live at the bar's right end (#6, revisioned). First added is rightmost.
                    #[cfg(target_os = "windows")]
                    {
                        if icons::button(ui, "x", 28.0, false, tl!("Close")).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                        let (glyph, tip) = if maximized { ("copy", tl!("Restore")) } else { ("square", tl!("Maximize")) };
                        if icons::button(ui, glyph, 28.0, false, tip).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }
                        if icons::button(ui, "minus", 28.0, false, tl!("Minimize")).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    }
                    let (icon, label) = match app.theme_preference {
                        ThemePreference::System => ("settings", tl!("Use system setting")),
                        ThemePreference::Light => ("sun", tl!("Light gray")),
                        ThemePreference::Dark => ("moon", tl!("Dark gray")),
                    };
                    let tip = format!("{}: {label}", tl!("Display theme"));
                    let response = icons::button(ui, icon, 28.0, false, &tip);
                    egui::Popup::menu(&response).show(|ui| theme_menu(app, ui));
                    if icons::button(ui, "circle-help", 28.0, false, tl!("Keyboard shortcuts")).clicked() {
                        app.dialog = Some(Dialog::Shortcuts);
                    }
                });
                ui.data_mut(|data| data.insert_temp(reserve_id, (before - ui.available_width()).max(0.0) + 4.0));
            });
        });
}

/// Both theme entry points use the same choices and command path.
fn theme_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    for (preference, command, label) in [
        (ThemePreference::System, "view.theme.system", "Use system setting"),
        (ThemePreference::Light, "view.theme.light", "Light gray"),
        (ThemePreference::Dark, "view.theme.dark", "Dark gray"),
    ] {
        if ui.radio(app.theme_preference == preference, tl!(label)).clicked() {
            app.execute(command);
            ui.close();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn tab(ui: &mut egui::Ui, t: &Tokens, icon: &str, name: &str, dirty: bool, active: bool, close: &mut Option<usize>, index: usize) -> egui::Response {
    let font = theme::regular(13.0);
    let label: String = if name.chars().count() > 28 { format!("{}…", name.chars().take(27).collect::<String>()) } else { name.to_string() };
    // Painted text only: the accessibility name below keeps the logical order.
    let label = crate::bidi::visual(&label).into_owned();
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), t.text).size().x);
    let (rect, resp) = ui.allocate_exact_size(vec2(text_w + 64.0, 30.0), Sense::click());
    let a11y = if dirty { format!("{name} (edited)") } else { name.to_string() };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &a11y));
    let bg = if active {
        t.chrome
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, bg);
    icons::paint(ui, Rect::from_min_size(rect.min + vec2(6.0, 7.0), vec2(16.0, 16.0)), icon, 15.0, if active { t.accent } else { t.text_muted });
    ui.painter().text(rect.min + vec2(28.0, rect.height() / 2.0), Align2::LEFT_CENTER, label, font, if active { t.text } else { t.text_muted });
    let x_rect = Rect::from_center_size(rect.right_center() - vec2(16.0, 0.0), vec2(20.0, 20.0));
    let x = ui.interact(x_rect, ui.id().with(("tabclose", index)), Sense::click());
    if x.hovered() {
        ui.painter().rect_filled(x_rect, CornerRadius::same(4), t.pressed);
    }
    // Unsaved changes: a dot where the close button sits, until the tab is hovered.
    if dirty && !resp.hovered() && !x.hovered() {
        ui.painter().circle_filled(x_rect.center(), 4.0, if active { t.text } else { t.text_muted });
    } else if active || resp.hovered() || x.hovered() {
        icons::paint(ui, x_rect, "x", 13.0, t.text_muted);
    }
    if x.clicked() {
        *close = Some(index);
    }
    let shown = crate::bidi::visual(name);
    resp.on_hover_text(if dirty { crate::i18n::fmt(tl!("{name} — unsaved changes"), &[("name", shown.as_ref())]) } else { shown.into_owned() })
}

pub fn mode_bar(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("mode_bar")
        .exact_size(48.0)
        // No bottom divider: the active tab's indicator is the bar's only line.
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(10, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                main_menu(app, ui);
                ui.add_space(6.0);
                ui.painter().vline(ui.cursor().left(), ui.max_rect().y_range().shrink(12.0), Stroke::new(1.0, t.divider));
                ui.add_space(10.0);
                for (mode, label) in
                    [(Mode::AllTools, "All tools"), (Mode::Read, "Read"), (Mode::Edit, "Edit"), (Mode::Convert, "Convert"), (Mode::Sign, "E-Sign")]
                {
                    if widgets::mode_tab(ui, tl!(label), app.mode == mode).clicked() {
                        app.select_mode(mode);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let has_doc = app.active.is_some();
                    ui.add_enabled_ui(has_doc, |ui| {
                        if icons::button(ui, "printer", 32.0, false, tl!("Print (⌘P)")).clicked() {
                            app.run_command("print.dialog");
                        }
                        if icons::button(ui, "save", 32.0, false, tl!("Save (M4)")).clicked() {
                            app.run_command("file.save");
                        }
                        if icons::button(ui, "info", 32.0, false, tl!("Document properties (⌘D)")).clicked() {
                            app.dialog = Some(Dialog::Properties(PropsTab::Description));
                        }
                    });
                    ui.add_space(8.0);
                    if widgets::search_box(ui, tl!("Find tools and commands"), 260.0).clicked() {
                        app.palette_open = true;
                    }
                });
            });
        });
}

fn main_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let resp = widgets::ghost_button(ui, "panel-left", tl!("Menu"));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(230.0);
        ui.menu_button(tl!("File"), |ui| crate::commands::registry_menu(app, ui, "File"));
        ui.menu_button(tl!("Edit"), |ui| crate::commands::registry_menu(app, ui, "Edit"));
        ui.menu_button(tl!("Pages"), |ui| crate::commands::registry_menu(app, ui, "Pages"));
        ui.menu_button(tl!("View"), |ui| {
            if let Some(i) = app.active {
                let v = &mut app.views[i];
                ui.label(egui::RichText::new(tl!("Zoom")).color(t.text_faint).small());
                if widgets::menu_item(ui, tl!("Actual size"), "⌘1").clicked() {
                    v.set_zoom(1.0);
                }
                if widgets::menu_item(ui, tl!("Zoom to page level"), "⌘0").clicked() {
                    v.fit = Fit::Page;
                }
                if widgets::menu_item(ui, tl!("Fit to width"), "⌘2").clicked() {
                    v.fit = Fit::Width;
                }
                if widgets::menu_item(ui, tl!("Fit to height"), "").clicked() {
                    v.fit = Fit::Height;
                    v.goto = Some((v.current, 0.0));
                }
                if widgets::menu_item(ui, tl!("Fit visible"), "⌘3").clicked() {
                    ui.close();
                    app.execute("view.fit_visible");
                    return;
                }
                if widgets::menu_item(ui, tl!("Zoom in"), "⌘+").clicked() {
                    v.zoom_step(true);
                }
                if widgets::menu_item(ui, tl!("Zoom out"), "⌘−").clicked() {
                    v.zoom_step(false);
                }
                if widgets::menu_item(ui, tl!("Rotate view clockwise"), "⇧⌘+").clicked() {
                    v.rotate_view(true);
                }
                if widgets::menu_item(ui, tl!("Rotate view counterclockwise"), "⇧⌘−").clicked() {
                    v.rotate_view(false);
                }
                ui.separator();
                ui.label(egui::RichText::new(tl!("Page navigation")).color(t.text_faint).small());
                if ui.add_enabled(!v.back.is_empty(), egui::Button::new(tl!("Previous view")).shortcut_text("⌘[")).clicked() {
                    v.view_history(false);
                }
                if ui.add_enabled(!v.forward.is_empty(), egui::Button::new(tl!("Next view")).shortcut_text("⌘]")).clicked() {
                    v.view_history(true);
                }
                ui.separator();
                if let Some(id) = page_display_menu(ui, &app.views[i]) {
                    app.execute(id);
                }
                ui.separator();
            }
            crate::commands::registry_menu(app, ui, "View");
            ui.menu_button(tl!("Display theme"), |ui| theme_menu(app, ui));
            ui.menu_button(tl!("Side panels"), |ui| {
                for (p, label) in [
                    (RightPanel::Comments, tl!("Comments")),
                    (RightPanel::Bookmarks, tl!("Bookmarks")),
                    (RightPanel::Pages, tl!("Pages")),
                    (RightPanel::Fields, tl!("Fields")),
                    (RightPanel::Layers, tl!("Layers")),
                    (RightPanel::Attachments, tl!("Attachments")),
                    (RightPanel::Signatures, tl!("Signatures")),
                    (RightPanel::Accessibility, tl!("Accessibility Checker")),
                    (RightPanel::Search, tl!("Search")),
                    (RightPanel::Compare, tl!("Compare")),
                ] {
                    if ui.radio(app.right == Some(p), tl!(label)).clicked() {
                        app.right = Some(p);
                    }
                }
            });
        });
        ui.menu_button(tl!("Help"), |ui| crate::commands::registry_menu(app, ui, "Help"));
    });
}

pub fn right_rail(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((index, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let has_signatures = doc.is_signed();
    let has_check = app.a11y.report.as_ref().is_some_and(|(d, _)| *d == id);
    let (has_comments, has_outline, has_fields, has_layers, has_files) = (
        !doc.info.annotations.is_empty(),
        !doc.info.outline.is_empty(),
        !doc.info.fields.is_empty(),
        !doc.info.layers.is_empty(),
        !doc.info.attachments.is_empty(),
    );
    let page_count = doc.info.pages.len();
    let labels: Vec<String> = doc.info.pages.iter().map(|p| p.label.clone()).collect();
    egui::Panel::right("rail")
        .resizable(false)
        .exact_size(48.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(6, 8)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let mut rail_button = |ui: &mut egui::Ui, panel: RightPanel, icon: &str, tip: &str, has: bool| {
                let selected = app.right == Some(panel);
                let r = icons::button(ui, icon, 34.0, selected, tl!(tip));
                if has && !selected {
                    let c = r.rect.right_top() + vec2(-8.0, 8.0);
                    ui.painter().circle_filled(c, 3.0, t.accent);
                }
                if r.clicked() {
                    app.right = if selected { None } else { Some(panel) };
                }
            };
            rail_button(ui, RightPanel::Comments, "message-square-text", "Comments", has_comments);
            rail_button(ui, RightPanel::Bookmarks, "bookmark", "Bookmarks", has_outline);
            rail_button(ui, RightPanel::Pages, "files", "Page thumbnails", false);
            rail_button(ui, RightPanel::Fields, "text-cursor-input", "Form fields", has_fields);
            rail_button(ui, RightPanel::Layers, "layers", "Layers", has_layers);
            rail_button(ui, RightPanel::Attachments, "paperclip", "Attachments", has_files);
            rail_button(ui, RightPanel::Signatures, "signature", "Signatures", has_signatures);
            if has_check {
                rail_button(ui, RightPanel::Accessibility, "accessibility", "Accessibility Checker", false);
            }

            // Page navigation cluster at the bottom (as in Acrobat's rail).
            let view = &mut app.views[index];
            let mut page_display = None;
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if icons::button(ui, "zoom-out", 32.0, false, tl!("Zoom out (⌘−)")).clicked() {
                    view.zoom_step(false);
                }
                if icons::button(ui, "zoom-in", 32.0, false, tl!("Zoom in (⌘+)")).clicked() {
                    view.zoom_step(true);
                }
                // Between rotate and zoom, as in Acrobat. Its command runs once `view` is free.
                let tip = crate::i18n::fmt(tl!("Page display: {layout}"), &[("layout", tl!(view.layout.label()))]);
                let resp = icons::button(ui, view.layout.icon(), 32.0, false, &tip);
                page_display = egui::Popup::menu(&resp)
                    .show(|ui| {
                        ui.set_min_width(220.0);
                        rail_view_menu(ui, view)
                    })
                    .and_then(|r| r.inner);
                if icons::button(ui, "rotate-cw", 32.0, false, tl!("Rotate view clockwise (⇧⌘+)")).clicked() {
                    view.rotate_view(true);
                }
                // Not `columns-2` while fitting the page: that is the two-page view's icon.
                let fit_icon = if view.fit == Fit::Width { "maximize" } else { "maximize-2" };
                if icons::button(ui, fit_icon, 32.0, false, tl!("Toggle fit page / fit width")).clicked() {
                    view.fit = if view.fit == Fit::Width { Fit::Page } else { Fit::Width };
                    view.goto = Some((view.current, 0.0));
                }
                ui.label(egui::RichText::new(format!("{:.0}%", view.zoom * 100.0)).font(theme::regular(10.5)).color(t.text_faint));
                ui.add_space(6.0);
                if icons::button(ui, "chevron-down", 30.0, false, tl!("Next page")).clicked() {
                    view.step_page(true);
                }
                if icons::button(ui, "chevron-up", 30.0, false, tl!("Previous page")).clicked() {
                    view.step_page(false);
                }
                ui.label(egui::RichText::new(page_count.to_string()).font(theme::regular(11.0)).color(t.text_muted));
                let edit = egui::TextEdit::singleline(&mut view.page_input)
                    .id(egui::Id::new("page-input"))
                    .desired_width(34.0)
                    .horizontal_align(Align::Center)
                    .font(theme::medium(12.0))
                    .margin(vec2(2.0, 4.0));
                let r = egui::Frame::NONE
                    .fill(t.field)
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(CornerRadius::same(5))
                    .show(ui, |ui| ui.add(edit))
                    .inner
                    .on_hover_text(tl!("Current page — type a page number or label (such as iv) and press Enter"));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    // A page label first (logical page numbers, as Acrobat), then a number.
                    let typed = view.page_input.clone();
                    if !view.go_to_typed(&typed, &labels) {
                        view.page_input = (view.current + 1).to_string();
                    }
                }
            });
            if let Some(id) = page_display {
                app.execute(id);
            }
        });
}

/// View ▸ Page display and the rail's Page display menu: the layouts and the cover page.
/// Returns the command a click asks for, for the caller to run.
fn page_display_menu(ui: &mut egui::Ui, view: &DocView) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tl!("Page display")).color(t.text_faint).small());
    let mut picked = None;
    for l in PageLayout::ORDER {
        if ui.radio(view.layout == l, tl!(l.label())).clicked() {
            picked = Some(l.command());
        }
    }
    let mut cover = view.cover;
    if ui.add_enabled(view.cover_applies(), egui::Checkbox::new(&mut cover, tl!("Show cover page in two-page view"))).changed() {
        picked = Some("view.layout.cover");
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// The rail's Page display menu: Acrobat's view choices around the page display. Zoom changes
/// apply at once; the command a click asks for is returned for the caller to run.
fn rail_view_menu(ui: &mut egui::Ui, view: &mut DocView) -> Option<&'static str> {
    let mut picked = None;
    for (id, label) in [("view.fit_width_scrolling", "Fit to width scrolling"), ("view.fit_one_page", "Fit one full page")] {
        if ui.button(tl!(label)).clicked() {
            picked = Some(id);
        }
    }
    ui.separator();
    if ui.button(tl!("Actual size")).clicked() {
        view.set_zoom(1.0);
        ui.close();
    }
    if ui.button(tl!("Zoom to page level")).clicked() {
        view.set_fit(Fit::Page);
        ui.close();
    }
    if ui.button(tl!("Fit visible")).clicked() {
        picked = Some("view.fit_visible");
    }
    ui.separator();
    picked = page_display_menu(ui, view).or(picked);
    ui.separator();
    for (id, label) in [("view.read_mode", "Read mode"), ("view.full_screen", "Full screen mode")] {
        if ui.button(tl!(label)).clicked() {
            picked = Some(id);
        }
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Grab width of the manual resize rim (Windows custom title bar), in points.
#[cfg(target_os = "windows")]
const RESIZE_RIM: f32 = 6.0;

/// A window edge (or corner) a manual resize drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[cfg(target_os = "windows")]
pub enum ResizeEdge {
    Left,
    Right,
    Bottom,
    BottomLeft,
    BottomRight,
}

/// Manual window-edge resize drag (Windows custom title bar only).
#[derive(Clone, Copy, Debug)]
#[cfg(target_os = "windows")]
pub struct WindowResize {
    edge: ResizeEdge,
    /// Pointer and window rect when the drag started (points).
    origin: Pos2,
    start: Rect,
    /// Last rect sent, so a still pointer sends nothing.
    sent: Rect,
}

/// New window rect when the pointer moved `delta` since the drag started. Left edges
/// move the position too; everything clamps to `min`. A non-finite delta (never seen
/// from input) resizes nothing rather than poisoning the rect.
#[cfg(target_os = "windows")]
pub fn resize_rect(start: Rect, edge: ResizeEdge, delta: Vec2, min: Vec2) -> Rect {
    let delta = Vec2::new(if delta.x.is_finite() { delta.x } else { 0.0 }, if delta.y.is_finite() { delta.y } else { 0.0 });
    let (mut lo, mut hi) = (start.min, start.max);
    if matches!(edge, ResizeEdge::Left | ResizeEdge::BottomLeft) {
        lo.x = (start.min.x + delta.x).min(start.max.x - min.x);
    }
    if matches!(edge, ResizeEdge::Right | ResizeEdge::BottomRight) {
        hi.x = (start.min.x + min.x).max(start.max.x + delta.x);
    }
    if !matches!(edge, ResizeEdge::Left | ResizeEdge::Right) {
        hi.y = (start.min.y + min.y).max(start.max.y + delta.y);
    }
    Rect { min: lo, max: hi }
}

/// Manual window-edge resize for the decoration-less Windows bar (#6, revisioned).
/// Skipped when maximized or fullscreen (nothing to resize). The strips hug the rim
/// outside the panels; a floating panel scrollbar may briefly underlap their outer
/// points while it is visible. The top edge stays with the tab strip (drag to move).
#[cfg(target_os = "windows")]
pub fn resize_edges(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if app.full_screen || ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        app.window_resize = None;
        return;
    }
    if let Some(drag) = app.window_resize {
        if !ctx.input(|i| i.pointer.primary_down()) {
            app.window_resize = None;
            return;
        }
        let (pos, _) = ctx.input(|i| (i.pointer.hover_pos(), i.viewport().inner_rect));
        if let Some(pos) = pos {
            let next = resize_rect(drag.start, drag.edge, pos - drag.origin, vec2(820.0, 520.0));
            if next != drag.sent {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(next.size()));
                if next.min != drag.start.min {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(next.min));
                }
                if let Some(active) = app.window_resize.as_mut() {
                    active.sent = next;
                }
            }
            ctx.request_repaint();
        }
        return;
    }
    let screen = ctx.viewport_rect();
    let zones = [
        (ResizeEdge::BottomLeft, Rect::from_min_size(pos2(0.0, screen.max.y - 14.0), vec2(14.0, 14.0)), CursorIcon::ResizeNeSw),
        (ResizeEdge::BottomRight, Rect::from_min_size(pos2(screen.max.x - 14.0, screen.max.y - 14.0), vec2(14.0, 14.0)), CursorIcon::ResizeNwSe),
        (ResizeEdge::Left, Rect::from_min_max(pos2(0.0, 0.0), pos2(RESIZE_RIM, screen.max.y)), CursorIcon::ResizeHorizontal),
        (ResizeEdge::Right, Rect::from_min_max(pos2(screen.max.x - RESIZE_RIM, 0.0), pos2(screen.max.x, screen.max.y)), CursorIcon::ResizeHorizontal),
        (ResizeEdge::Bottom, Rect::from_min_max(pos2(0.0, screen.max.y - RESIZE_RIM), pos2(screen.max.x, screen.max.y)), CursorIcon::ResizeVertical),
    ];
    egui::Area::new(egui::Id::new("window-resize")).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(ctx, |ui| {
        for (edge, rect, cursor) in zones {
            let response = ui.interact(rect, ui.id().with(("resize", edge as u8)), Sense::drag());
            if response.hovered() {
                ui.ctx().set_cursor_icon(cursor);
            }
            if response.drag_started()
                && let (Some(origin), Some(start)) = (response.interact_pointer_pos(), ctx.input(|i| i.viewport().inner_rect))
            {
                app.window_resize = Some(WindowResize { edge, origin, start, sent: start });
            }
        }
    });
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(100.0, 100.0), vec2(800.0, 600.0))
    }

    #[test]
    fn right_edge_grows_and_clamps_to_the_minimum() {
        let grown = resize_rect(rect(), ResizeEdge::Right, vec2(50.0, 0.0), vec2(820.0, 520.0));
        assert_eq!((grown.min, grown.size()), (rect().min, vec2(850.0, 600.0)));
        let shrunk = resize_rect(rect(), ResizeEdge::Right, vec2(-500.0, 0.0), vec2(820.0, 520.0));
        assert_eq!(shrunk.size(), vec2(820.0, 600.0), "never below the minimum");
        assert_eq!(shrunk.min, rect().min, "the position never moves");
    }

    #[test]
    fn left_edge_moves_and_clamps_to_the_minimum() {
        let grown = resize_rect(rect(), ResizeEdge::Left, vec2(-40.0, 0.0), vec2(820.0, 520.0));
        assert_eq!((grown.min.x, grown.size().x), (60.0, 840.0));
        let shrunk = resize_rect(rect(), ResizeEdge::Left, vec2(500.0, 0.0), vec2(820.0, 520.0));
        assert_eq!((shrunk.min.x, shrunk.size().x), (80.0, 820.0), "stops at the minimum width");
    }

    #[test]
    fn bottom_and_corners_move_both_axes() {
        // The minimum sits below the test rect, so clamping never kicks in here.
        let min = vec2(700.0, 500.0);
        let down = resize_rect(rect(), ResizeEdge::Bottom, vec2(0.0, 30.0), min);
        assert_eq!(down.size(), vec2(800.0, 630.0));
        let corner = resize_rect(rect(), ResizeEdge::BottomRight, vec2(10.0, 20.0), min);
        assert_eq!((corner.min, corner.size()), (rect().min, vec2(810.0, 620.0)));
        let other = resize_rect(rect(), ResizeEdge::BottomLeft, vec2(-10.0, 20.0), min);
        assert_eq!((other.min.x, other.size()), (90.0, vec2(810.0, 620.0)));
    }

    #[test]
    fn non_finite_deltas_resize_nothing() {
        // A minimum below the test rect, so only the damaged delta matters.
        let min = vec2(100.0, 100.0);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(resize_rect(rect(), ResizeEdge::BottomRight, vec2(bad, bad), min), rect());
        }
    }
}
