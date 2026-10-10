#[test]
fn editing_existing_images_on_the_page() {
    // A page made from a 40 × 20 image (at 72 dpi: a 40 × 20 pt page filled by it).
    let mut png = Vec::new();
    image::RgbImage::from_pixel(80, 40, image::Rgb([200, 40, 40])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.open_bytes("picture.png", None, png.clone()).expect("opens");
        app
    });
    h.run_steps(4);
    let id = h.state().views[0].id;
    let before = h.state().session.get(id).unwrap().page_images(0)[0].rect;
    assert!(h.state_mut().execute("edit.edit_text"));
    h.run_steps(2);
    let r = h.state().views[0].page_screen_rect(0).expect("on screen");
    // Select it, then drag it a quarter of the page to the right.
    let c = r.center();
    h.hover_at(c);
    h.run_steps(1);
    h.drag_at(c);
    h.run_steps(1);
    h.drop_at(c);
    h.run_steps(2);
    assert!(h.state().views[0].image_selection.is_some(), "selected");
    let to = c + egui::vec2(r.width() / 4.0, 0.0);
    h.hover_at(c);
    h.run_steps(1);
    h.drag_at(c);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(c + (to - c) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(4);
    {
        let doc = h.state().session.get(id).unwrap();
        assert_eq!(doc.can_undo(), Some("Move image"));
        let moved = doc.page_images(0)[0].rect;
        let dx = moved[0] - before[0];
        assert!((dx - (before[2] - before[0]) / 4.0).abs() < 1.5, "moved {dx} pt: {before:?} → {moved:?}");
    }
    // Delete with the Delete key.
    h.key_press(egui::Key::Delete);
    h.run_steps(4);
    assert!(h.state().session.get(id).unwrap().page_images(0).is_empty());
}
