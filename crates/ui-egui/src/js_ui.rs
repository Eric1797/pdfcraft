//! Acrobat JavaScript in the shell: what scripts ask for (alerts, printing, navigation, links),
//! the JavaScript console (⌘J), Document JavaScripts, and Preferences ▸ JavaScript.

use egui::{Align, Layout};
use pdfcraft_engine::js::{JsOutput, Request};
use pdfcraft_engine::{DocId, Edit};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, widgets};

/// The JavaScript console: the input and the output so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsConsole {
    pub input: String,
    pub log: Vec<String>,
}

/// Document JavaScripts: the script being edited.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocJsDraft {
    pub name: String,
    pub script: String,
}

impl PdfCraftApp {
    /// Act on what scripts produced in document `id`: alerts are shown, console output goes to
    /// the console, print and page requests are carried out, links wait for the user's permission
    /// (form submissions are reported, never sent).
    pub fn handle_js(&mut self, id: DocId, out: JsOutput) {
        if out.is_empty() {
            return;
        }
        self.js_console.log.extend(out.console.iter().cloned());
        for e in &out.errors {
            self.js_console.log.push(format!("Error: {e}"));
        }
        let view = self.views.iter().position(|v| v.id == id);
        for r in out.requests {
            match r {
                Request::Print => self.open_print(),
                Request::GoToPage(p) => {
                    if let Some(i) = view {
                        self.views[i].go_to_page(p);
                    }
                }
                Request::LaunchUrl(u) => self.request_document_url(&u, crate::LinkOrigin::Script),
                Request::Submit(u) => self.notify_fmt(
                    "The form asks to be submitted to {u}; PdfCraft doesn't send form data. Save the document to keep your entries.",
                    &[("u", &u)],
                ),
                Request::SaveAs => self.run_command("file.save_as"),
                Request::Focus(_) | Request::Beep | Request::Reset(_) => {}
            }
        }
        if let Some(a) = out.alerts.last() {
            self.notify(a.clone());
        }
    }

    /// Run a push button's JavaScript (its Mouse Up action).
    pub fn run_button_script(&mut self, id: DocId, field: &str, script: &str) {
        // A script reads the fields: include what's still being typed in one (#166).
        if !self.commit_form_typing() {
            return;
        }
        match self.session.run_javascript(id, script, Some(field)) {
            Ok(o) => {
                if let Some(i) = self.views.iter().position(|v| v.id == id)
                    && let Some(info) = self.session.get(id).map(|d| d.info.clone())
                {
                    self.views[i].document_changed(&info);
                }
                let out = JsOutput { alerts: o.alerts, console: o.console, requests: o.requests, errors: o.error.into_iter().collect() };
                self.handle_js(id, out);
            }
            Err(e) => self.notify_fmt("{field}: {e}", &[("field", field), ("e", &e.to_string())]),
        }
    }

    /// Prepare a form ▸ detect fields from the page's blanks, lines and boxes.
    pub fn detect_fields(&mut self) {
        let Some((i, id)) = self.active_ids() else { return };
        match self.session.auto_detect_fields(id, &[]) {
            Ok(names) if names.is_empty() => self.notify_tr("No form fields were detected"),
            Ok(names) => {
                if let Some(info) = self.session.get(id).map(|d| d.info.clone()) {
                    self.views[i].document_changed(&info);
                }
                if names.len() == 1 {
                    self.notify_tr("Detected 1 form field");
                } else {
                    self.notify_fmt("Detected {n} form fields", &[("n", &names.len().to_string())]);
                }
            }
            Err(e) => self.notify_error(e),
        }
    }

    /// The console's Run: evaluate the input in the active document.
    pub fn run_console(&mut self) {
        let Some((i, id)) = self.active_ids() else { return };
        let script = self.js_console.input.clone();
        if script.trim().is_empty() {
            return;
        }
        self.js_console.log.push(format!("> {}", script.trim()));
        match self.session.run_javascript(id, &script, None) {
            Ok(o) => {
                if let Some(info) = self.session.get(id).map(|d| d.info.clone()) {
                    self.views[i].document_changed(&info);
                }
                let (error, result) = (o.error.clone(), o.result.clone());
                let out = JsOutput { alerts: o.alerts, console: o.console, requests: o.requests, errors: Vec::new() };
                self.handle_js(id, out);
                match error {
                    Some(e) => self.js_console.log.push(format!("Error: {e}")),
                    None => self.js_console.log.extend(result),
                }
            }
            Err(e) => self.js_console.log.push(format!("Error: {e}")),
        }
    }
}

fn buttons(ui: &mut egui::Ui, primary: &str, others: &[&str]) -> Option<String> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Stable ids: the console's output above changes how many widgets come first.
            // Labels are translated for display; the returned id stays English.
            if ui.push_id(primary, |ui| widgets::pill_button(ui, tl!(primary), true)).inner.clicked() {
                clicked = Some(primary.to_string());
            }
            for o in others {
                if ui.push_id(o, |ui| widgets::pill_button(ui, tl!(o), false)).inner.clicked() {
                    clicked = Some(o.to_string());
                }
            }
        })
    });
    clicked
}

/// The JavaScript console. Returns `true` to close.
pub(crate) fn console_body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) -> bool {
    ui.label(egui::RichText::new(tl!("JavaScript Console")).font(theme::semibold(18.0)));
    ui.add_space(6.0);
    if !app.session.javascript() {
        ui.label(egui::RichText::new(tl!("JavaScript is turned off (Preferences ▸ JavaScript).")).small().color(t.text_muted));
    }
    egui::Frame::new().fill(t.hover).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(160.0);
            if app.js_console.log.is_empty() {
                ui.label(egui::RichText::new(tl!("Output appears here.")).color(t.text_muted));
            }
            for line in &app.js_console.log {
                ui.label(egui::RichText::new(line).monospace());
            }
        });
    });
    ui.add_space(6.0);
    let input = ui.add(
        egui::TextEdit::multiline(&mut app.js_console.input)
            .code_editor()
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .hint_text(tl!("JavaScript, e.g. getField(\"total\").value"))
            .id_salt("js-console-input"),
    );
    let run_key = input.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    ui.add_space(8.0);
    match buttons(ui, "Run", &["Close", "Clear"]).as_deref() {
        Some("Run") => app.run_console(),
        Some("Clear") => app.js_console.log.clear(),
        Some("Close") => return true,
        _ if run_key => app.run_console(),
        _ => {}
    }
    false
}

/// Document JavaScripts: list, edit, add and delete. Returns `true` to close.
pub(crate) fn document_js_body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) -> bool {
    ui.label(egui::RichText::new(tl!("Document JavaScripts")).font(theme::semibold(18.0)));
    ui.add_space(6.0);
    let scripts = app.active_ids().and_then(|(_, id)| app.session.get(id)).map(|d| d.document_scripts()).unwrap_or_default();
    let mut edit: Option<Edit> = None;
    ui.horizontal(|ui| {
        ui.label(tl!("Script Name:"));
        ui.add(egui::TextEdit::singleline(&mut app.doc_js.name).desired_width(240.0).id_salt("doc-js-name"));
    });
    ui.add_space(4.0);
    egui::Frame::new().fill(t.hover).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(120.0).id_salt("doc-js-list").show(ui, |ui| {
            ui.set_width(ui.available_width());
            if scripts.is_empty() {
                ui.label(egui::RichText::new(tl!("This document has no document-level scripts.")).color(t.text_muted));
            }
            for (name, js) in &scripts {
                if ui.selectable_label(app.doc_js.name == *name, name).clicked() {
                    app.doc_js = DocJsDraft { name: name.clone(), script: js.clone() };
                }
            }
        });
    });
    ui.add_space(6.0);
    ui.add(egui::TextEdit::multiline(&mut app.doc_js.script).code_editor().desired_rows(8).desired_width(f32::INFINITY).id_salt("doc-js-script"));
    ui.add_space(8.0);
    let name = app.doc_js.name.trim().to_string();
    let close = match buttons(ui, "Save", &["Close", "Delete"]).as_deref() {
        Some("Save") if !name.is_empty() => {
            edit = Some(Edit::SetDocumentScript { name, script: Some(app.doc_js.script.clone()) });
            false
        }
        Some("Delete") if scripts.iter().any(|(n, _)| *n == name) => {
            edit = Some(Edit::SetDocumentScript { name, script: None });
            app.doc_js = DocJsDraft::default();
            false
        }
        Some("Close") => true,
        _ => false,
    };
    if let Some(e) = edit {
        app.apply_edit(e);
    }
    close
}

/// Preferences: interface language, identity and JavaScript. Returns `true` to close.
pub(crate) fn preferences_body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) -> bool {
    ui.label(egui::RichText::new(tl!("Preferences")).font(theme::semibold(18.0)));
    ui.horizontal(|ui| {
        ui.label(tl!("Interface language"));
        let selected = crate::i18n::Lang::from_code(&app.language).map_or(tl!("Auto"), crate::i18n::Lang::name);
        let before = app.language.clone();
        egui::ComboBox::from_id_salt("interface-language").selected_text(selected).show_ui(ui, |ui| {
            if ui.selectable_value(&mut app.language, crate::i18n::AUTO.to_string(), tl!("Auto")).clicked() {
                ui.close();
            }
            for language in crate::i18n::Lang::all() {
                if ui.selectable_value(&mut app.language, language.code().to_string(), language.name()).clicked() {
                    ui.close();
                }
            }
        });
        // Relabel the rest of this dialog in the new language right away, not next frame.
        if app.language != before {
            crate::i18n::set_current(crate::i18n::Lang::from_pref(&app.language));
        }
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tl!("Documents and view")).font(theme::semibold(13.0)));
    ui.horizontal(|ui| {
        ui.label(tl!("Default workspace mode"));
        for (mode, label) in [
            (crate::Mode::AllTools, "All tools"),
            (crate::Mode::Read, "Read"),
            (crate::Mode::Edit, "Edit"),
            (crate::Mode::Convert, "Convert"),
            (crate::Mode::Sign, "E-Sign"),
        ] {
            ui.radio_value(&mut app.default_mode, mode, tl!(label));
        }
    });
    ui.label(
        egui::RichText::new(tl!("Used when opening PDFs. An explicit launch or control mode takes precedence for the session."))
            .small()
            .color(t.text_muted),
    );
    let defaults = &mut app.view_defaults;
    ui.horizontal(|ui| {
        ui.label(tl!("Default page display"));
        for l in crate::canvas::PageLayout::ORDER {
            ui.radio_value(&mut defaults.layout, l, tl!(l.label()));
        }
    });
    ui.horizontal(|ui| {
        use crate::canvas::Fit;
        ui.label(tl!("Default zoom"));
        ui.radio_value(&mut defaults.fit, Fit::Width, tl!("Fit to width"));
        ui.radio_value(&mut defaults.fit, Fit::Page, tl!("Zoom to page level"));
        ui.radio_value(&mut defaults.fit, Fit::None, tl!("Custom"));
        // Editing the percentage selects Custom.
        let mut percent = defaults.zoom * 100.0;
        if ui.add(egui::DragValue::new(&mut percent).range(8.0..=6400.0).max_decimals(0).suffix("%")).changed() {
            (defaults.fit, defaults.zoom) = (Fit::None, percent / 100.0);
        }
    });
    ui.label(
        egui::RichText::new(tl!("Used when a PDF doesn't ask for a layout or zoom. Continuous scrolling never snaps between pages."))
            .small()
            .color(t.text_muted),
    );
    ui.add_space(8.0);
    // Identity: the author of new comments (Acrobat: Preferences ▸ Identity).
    ui.label(egui::RichText::new(tl!("Identity")).font(theme::semibold(13.0)));
    ui.horizontal(|ui| {
        let label = ui.label(tl!("Name on new comments"));
        ui.add(egui::TextEdit::singleline(&mut app.comment_prefs.author).desired_width(220.0).char_limit(crate::MAX_AUTHOR_CHARS))
            .labelled_by(label.id);
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tl!("Customize program")).font(theme::semibold(13.0)));
    ui.horizontal(|ui| {
        let label = ui.label(tl!("App name"));
        ui.add(egui::TextEdit::singleline(&mut app.custom_name).desired_width(220.0).char_limit(crate::MAX_CUSTOM_NAME_CHARS)).labelled_by(label.id);
    });
    ui.label(egui::RichText::new(tl!("Window and taskbar title; empty keeps PdfCraft.")).small().color(t.text_muted));
    ui.horizontal(|ui| {
        ui.label(tl!("App icon"));
        #[cfg(not(target_arch = "wasm32"))]
        if ui.button(tl!("Choose PNG…")).clicked() {
            let dialog = rfd::AsyncFileDialog::new().add_filter("PNG image", &["png"]).add_filter("Icon", &["ico"]);
            app.ask_one(crate::pickers::Ask::File(dialog), None, |app, path| app.set_custom_icon(path));
        }
        if ui.button(tl!("Default")).clicked() {
            app.custom_icon_path = None;
        }
        let status = match app.custom_icon_path.as_deref().and_then(|p| std::path::Path::new(p).file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => tl!("Built-in icon").to_string(),
        };
        ui.label(status);
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new("JavaScript").font(theme::semibold(13.0)));
    egui::Frame::new().fill(t.hover).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut on = app.session.javascript();
        if ui.checkbox(&mut on, tl!("Enable Acrobat JavaScript")).changed() {
            app.session.set_javascript(on);
        }
        ui.label(
            egui::RichText::new(tl!("Scripts run in a sandbox without file or network access. A script that asks to open a web page needs your permission first. With JavaScript off, Acrobat's standard format, validate and calculate functions still work."))
                .small()
                .color(t.text_muted),
        );
    });
    ui.add_space(10.0);
    buttons(ui, tl!("OK"), &[]).is_some()
}

/// Largest icon file accepted (icons are tiny; anything bigger is a mistake).
const MAX_ICON_BYTES: usize = 1024 * 1024;
/// Largest icon dimension in pixels (Windows icons top out at 256).
const MAX_ICON_DIM: u32 = 512;

/// Check PNG bytes far enough to trust decoding them: PNG signature plus IHDR
/// dimensions within bounds, so a corrupt or hostile file cannot make the decoder
/// allocate blindly. Decoding itself can still fail; handled where it is used.
fn icon_png_dims(png: &[u8]) -> Result<(u32, u32), String> {
    if png.len() > MAX_ICON_BYTES {
        return Err(format!("larger than {} KiB", MAX_ICON_BYTES / 1024));
    }
    let head = png.get(..24).ok_or_else(|| "not a PNG image".to_string())?;
    if head.get(..8) != Some(b"\x89PNG\r\n\x1a\n".as_slice()) || head.get(12..16) != Some(b"IHDR".as_slice()) {
        return Err("not a PNG image".to_string());
    }
    let dim = |range: std::ops::Range<usize>| {
        head.get(range).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes).filter(|d| (1..=MAX_ICON_DIM).contains(d))
    };
    match (dim(16..20), dim(20..24)) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err("unsupported image size".to_string()),
    }
}

/// Decode a picked icon file (PNG, or ICO with PNG or 32-bit direct-color entries)
/// into window-icon pixels. Everything is bounds-checked; hostile files are refused
/// with a reason, never partially read.
fn decode_icon_file(bytes: &[u8]) -> Result<egui::IconData, String> {
    if bytes.len() > MAX_ICON_BYTES {
        return Err(format!("larger than {} KiB", MAX_ICON_BYTES / 1024));
    }
    if bytes.get(..8) == Some(b"\x89PNG\r\n\x1a\n".as_slice()) {
        icon_png_dims(bytes)?;
        return eframe::icon_data::from_png_bytes(bytes).map_err(|e| e.to_string());
    }
    let (width, height, rgba) = ico_rgba(bytes)?;
    Ok(egui::IconData { width, height, rgba })
}

/// Pixel data of an .ico file's best entry: the largest PNG entry, else the largest
/// 32-bit direct-color entry (with its transparency mask), within bounds.
fn ico_rgba(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let bad = || "bad icon file".to_string();
    let dir = data.get(..6).ok_or_else(bad)?;
    if dir.get(..4) != Some([0u8, 0, 1, 0].as_slice()) {
        return Err("not an icon file".to_string());
    }
    let count = u16::from_le_bytes([dir[4], dir[5]]) as usize;
    if count == 0 || count > 64 {
        return Err(bad());
    }
    // (area, width, height, offset, end, png, bits per pixel).
    let mut best_png: Option<(u32, u32, u32, usize, usize)> = None;
    let mut best_dib: Option<(u32, u32, u32, usize, usize)> = None;
    for i in 0..count {
        let entry = data.get(6 + i * 16..6 + (i + 1) * 16).ok_or_else(bad)?;
        let dim = |b: u8| if b == 0 { 256 } else { b as u32 };
        let (w, h) = (dim(entry[0]), dim(entry[1]));
        let bpp = u16::from_le_bytes([entry[4], entry[5]]);
        let size = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        let end = offset.checked_add(size).ok_or_else(bad)?;
        let body = data.get(offset..end).ok_or_else(bad)?;
        let png = body.get(..8) == Some(b"\x89PNG\r\n\x1a\n".as_slice());
        let area = w.saturating_mul(h);
        let better = |best: &Option<(u32, u32, u32, usize, usize)>| area > best.map_or(0, |(a, _, _, _, _)| a);
        if png && better(&best_png) {
            best_png = Some((area, w, h, offset, end));
        } else if !png && bpp == 32 && better(&best_dib) {
            best_dib = Some((area, w, h, offset, end));
        }
    }
    if let Some((_, _, _, offset, end)) = best_png {
        let entry = data.get(offset..end).ok_or_else(bad)?;
        icon_png_dims(entry)?;
        return eframe::icon_data::from_png_bytes(entry).map(|icon| (icon.width, icon.height, icon.rgba)).map_err(|_| "unreadable image".to_string());
    }
    if let Some((_, w, h, offset, end)) = best_dib {
        let body = data.get(offset..end).ok_or_else(bad)?;
        return dib32_rgba(body, w, h);
    }
    Err("no usable image in the icon".to_string())
}

/// 32-bit direct-color DIB pixels (with transparency mask) as RGBA, top-down.
fn dib32_rgba(body: &[u8], width: u32, height: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let bad = || "bad icon file".to_string();
    let head = body.get(..40).ok_or_else(bad)?;
    let u32le = |r: std::ops::Range<usize>| head.get(r).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    let u16le = |r: std::ops::Range<usize>| head.get(r).and_then(|b| <[u8; 2]>::try_from(b).ok()).map(u16::from_le_bytes);
    let (size, bw, bh, planes, bpp, comp) = match (u32le(0..4), head.get(4..12), u16le(12..14), u16le(14..16), u32le(16..20)) {
        (Some(size), Some(wh), Some(planes), Some(bpp), Some(comp)) => {
            let w = i32::from_le_bytes([wh[0], wh[1], wh[2], wh[3]]);
            let h = i32::from_le_bytes([wh[4], wh[5], wh[6], wh[7]]);
            (size, w, h, planes, bpp, comp)
        }
        _ => return Err(bad()),
    };
    // Uncompressed 32-bit, with the doubled height (pixels plus mask) the format stores.
    if size != 40 || planes != 1 || bpp != 32 || comp != 0 || bw <= 0 || bh <= 0 || bh % 2 != 0 {
        return Err("unsupported icon image".to_string());
    }
    if bw as u32 != width || (bh / 2) as u32 != height {
        return Err(bad());
    }
    let stride = (width as usize).checked_mul(4).ok_or_else(bad)?;
    let total = stride.checked_mul(height as usize).ok_or_else(bad)?;
    let and_stride = (((width + 31) / 32) * 4) as usize;
    let and_len = and_stride.checked_mul(height as usize).ok_or_else(bad)?;
    let px_end = 40usize.checked_add(total).ok_or_else(bad)?;
    let mask_end = px_end.checked_add(and_len).ok_or_else(bad)?;
    let px = body.get(40..px_end).ok_or_else(bad)?;
    let mask = body.get(px_end..mask_end).ok_or_else(bad)?;
    // Pixels are bottom-up BGRA; the mask (MSB first) clears transparent pixels.
    // Output runs top-down RGBA.
    let mut rgba = Vec::with_capacity(total);
    for out_y in 0..height as usize {
        let buf_y = height as usize - 1 - out_y;
        for x in 0..width as usize {
            let p = px.get(buf_y * stride + x * 4..buf_y * stride + x * 4 + 4).ok_or_else(bad)?;
            let transparent = mask.get(buf_y * and_stride + x / 8).is_some_and(|m| m & (0x80 >> (x % 8)) != 0);
            let transparent_px = [0, 0, 0, 0];
            let opaque_px = [p[2], p[1], p[0], p[3]];
            rgba.extend_from_slice(if transparent { &transparent_px } else { &opaque_px });
        }
    }
    Ok((width, height, rgba))
}

impl PdfCraftApp {
    /// Take a picked icon file (PNG, or ICO with PNG or 32-bit entries) as the
    /// window and taskbar icon (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_custom_icon(&mut self, path: std::path::PathBuf) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(&path) {
            Ok(bytes) => match decode_icon_file(&bytes) {
                Ok(_) => {
                    self.custom_icon_path = Some(path.to_string_lossy().into_owned());
                }
                Err(e) => self.notify_fmt("Couldn't use {name}: {e}", &[("name", &name), ("e", &e)]),
            },
            Err(e) => self.notify_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e.to_string())]),
        }
    }

    /// Apply a changed custom icon (Preferences ▸ Customize program): decode and send
    /// it as the window and taskbar icon, or say why it cannot be used. Runs when the
    /// path changes; a missing file falls back to the built-in icon.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn apply_custom_icon(&mut self, ctx: &egui::Context) {
        if self.custom_icon_applied == self.custom_icon_path {
            return;
        }
        self.custom_icon_applied.clone_from(&self.custom_icon_path);
        let Some(path) = self.custom_icon_path.clone() else { return };
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = std::fs::read(&path).unwrap_or_default();
        match decode_icon_file(&bytes) {
            Ok(icon) => ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(icon)))),
            Err(e) => self.notify_fmt("Couldn't use {name} as the app icon: {e}", &[("name", &name), ("e", &e)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header with the given IHDR dimensions (the validator reads this far only).
    fn png_head(width: u32, height: u32) -> Vec<u8> {
        let mut head = b"\x89PNG\r\n\x1a\n".to_vec();
        head.extend_from_slice(&13u32.to_be_bytes());
        head.extend_from_slice(b"IHDR");
        head.extend_from_slice(&width.to_be_bytes());
        head.extend_from_slice(&height.to_be_bytes());
        head
    }

    #[test]
    fn icon_dims_accept_a_sane_png() {
        assert_eq!(icon_png_dims(&png_head(64, 64)), Ok((64, 64)));
    }

    #[test]
    fn icon_dims_reject_garbage_and_absurd_sizes() {
        assert!(icon_png_dims(b"hello").is_err(), "not a PNG");
        assert!(icon_png_dims(&png_head(0, 64)).is_err(), "empty dimension");
        assert!(icon_png_dims(&png_head(10_000, 10)).is_err(), "oversized dimension");
        assert!(icon_png_dims(&vec![0u8; MAX_ICON_BYTES + 1]).is_err(), "oversized file");
    }

    /// A minimal .ico: directory, one 2x2 32-bit entry (pixels plus mask), no CRCs anywhere.
    fn ico_dib() -> Vec<u8> {
        let px: [[u8; 4]; 4] = [[10, 20, 30, 255], [40, 50, 60, 255], [70, 80, 90, 255], [100, 110, 120, 255]];
        let mut ico = vec![0, 0, 1, 0, 1, 0, 2, 2, 0, 0, 1, 0, 32, 0];
        ico.extend_from_slice(&64u32.to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        let mut info = 40u32.to_le_bytes().to_vec();
        info.extend_from_slice(&2i32.to_le_bytes());
        info.extend_from_slice(&4i32.to_le_bytes());
        info.extend_from_slice(&1u16.to_le_bytes());
        info.extend_from_slice(&32u16.to_le_bytes());
        info.extend_from_slice(&[0u8; 16]);
        let mut body = info;
        // Bottom-up BGRA, then an empty (opaque) mask.
        body.extend_from_slice(&px[2]);
        body.extend_from_slice(&px[3]);
        body.extend_from_slice(&px[0]);
        body.extend_from_slice(&px[1]);
        body.extend_from_slice(&[0u8; 8]);
        assert_eq!(body.len(), 64);
        ico.extend_from_slice(&body);
        ico
    }

    #[test]
    fn icon_files_decode_dib_entries_top_down_rgba() {
        let (w, h, rgba) = decode_icon_file(&ico_dib()).expect("2x2 direct color decodes");
        assert_eq!((w, h), (2, 2));
        // Top row first, BGRA flipped to RGBA, opaque mask kept.
        assert_eq!(rgba, vec![30, 20, 10, 255, 60, 50, 40, 255, 90, 80, 70, 255, 120, 110, 100, 255]);
    }

    #[test]
    fn icon_files_reject_garbage() {
        assert!(decode_icon_file(b"hello").is_err());
        assert!(decode_icon_file(&[0, 0, 1, 0]).is_err(), "truncated directory");
        assert!(decode_icon_file(&vec![0u8; MAX_ICON_BYTES + 1]).is_err(), "oversized file");
        // PNG magic with a corrupt body fails cleanly (no real PNG is embedded here).
        let mut bad = b"\x89PNG\r\n\x1a\n".to_vec();
        bad.extend_from_slice(&[0u8; 32]);
        assert!(decode_icon_file(&bad).is_err());
    }
}
