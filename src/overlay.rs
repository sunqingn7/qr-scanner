use eframe::egui;
use image::DynamicImage;
use screenshots::Screen;
use std::sync::mpsc;

pub enum OverlayResult {
    SelectedImage(DynamicImage),
    Cancelled,
    Error(String),
}

pub fn run_overlay() -> OverlayResult {
    eprintln!("run_overlay: starting...");
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let result = run_overlay_impl();
        let _ = tx.send(result);
    });

    let r = rx.recv().unwrap_or(OverlayResult::Cancelled);
    r
}

fn run_overlay_impl() -> OverlayResult {
    let screens = match Screen::all() {
        Ok(s) => s,
        Err(e) => return OverlayResult::Error(format!("Failed to get screens: {}", e)),
    };

    if screens.is_empty() {
        return OverlayResult::Error("No screens found".to_string());
    }

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for screen in &screens {
        min_x = min_x.min(screen.display_info.x);
        min_y = min_y.min(screen.display_info.y);
        max_x = max_x.max(screen.display_info.x + screen.display_info.width as i32);
        max_y = max_y.max(screen.display_info.y + screen.display_info.height as i32);
    }

    eprintln!("Overlay: screens from {}x{} to {}x{}", min_x, min_y, max_x, max_y);

    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let result_clone = result.clone();
    let screens_clone = screens.clone();

    struct OverlayApp {
        result: std::sync::Arc<std::sync::Mutex<Option<OverlayResult>>>,
        screens: Vec<Screen>,
        min_x: i32,
        min_y: i32,
    }

    impl eframe::App for OverlayApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            let screen_rect = ctx.screen_rect();

            let painter = ctx.layer_painter(egui::LayerId::background());
            painter.rect_filled(screen_rect, 0.0, egui::Color32::from_rgba_premultiplied(0, 0, 0, 150));

            #[derive(Default, Clone)]
            struct SelectionState {
                start: Option<egui::Pos2>,
                end: Option<egui::Pos2>,
                dragging: bool,
            }

            let mut state: SelectionState = ctx.data_mut(|d| {
                d.get_temp(egui::Id::new("selection")).unwrap_or_default()
            });

            let input = ctx.input(|i| i.clone());

            if input.pointer.any_pressed() && input.pointer.button_pressed(egui::PointerButton::Primary) {
                if let Some(pos) = input.pointer.press_origin() {
                    state.start = Some(pos);
                    state.end = Some(pos);
                    state.dragging = true;
                    eprintln!("Selection started at {:?}", pos);
                }
            }

            if state.dragging && input.pointer.any_released() {
                state.dragging = false;

                if let (Some(start), Some(end)) = (state.start, state.end) {
                    let rect = egui::Rect::from_two_pos(start, end);

                    if rect.width() > 20.0 && rect.height() > 20.0 {
                        eprintln!("Selection made: {:?}", rect);

                        let x = (rect.min.x - self.min_x as f32) as i32;
                        let y = (rect.min.y - self.min_y as f32) as i32;
                        let w = rect.width() as u32;
                        let h = rect.height() as u32;

                        for screen in &self.screens {
                            let sx = screen.display_info.x;
                            let sy = screen.display_info.y;
                            let sw = screen.display_info.width as i32;
                            let sh = screen.display_info.height as i32;

                            if x + w as i32 > sx && x < sx + sw && y + h as i32 > sy && y < sy + sh {
                                let capture_x = (x - sx).max(0);
                                let capture_y = (y - sy).max(0);
                                let capture_w = w.min((sx + sw - x) as u32);
                                let capture_h = h.min((sy + sh - y) as u32);

                                    if capture_w > 0 && capture_h > 0 {
                                    match screen.capture_area(capture_x, capture_y, capture_w, capture_h) {
                                        Ok(img) => {
                                            let width = img.width();
                                            let height = img.height();
                                            let raw = img.into_raw();
                                            if let Some(buf) = image::ImageBuffer::from_raw(width, height, raw) {
                                                let dyn_img = image::DynamicImage::ImageRgba8(buf);
                                                *self.result.lock().unwrap() = Some(OverlayResult::SelectedImage(dyn_img));
                                                eprintln!("Image captured!");
                                            }
                                        }
                                        Err(e) => {
                                            *self.result.lock().unwrap() = Some(OverlayResult::Error(format!("Capture failed: {}", e)));
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                }

                ctx.data_mut(|d| d.remove_temp::<SelectionState>(egui::Id::new("selection")));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }

            if state.dragging {
                if let Some(pos) = input.pointer.interact_pos() {
                    state.end = Some(pos);
                }
            }

            if let (Some(start), Some(end)) = (state.start, state.end) {
                let rect = egui::Rect::from_two_pos(start, end);
                let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("selection"));
                let painter = ctx.layer_painter(layer);
                painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_premultiplied(255, 255, 255, 30));
                painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::GREEN), egui::StrokeKind::Middle);
            }

            if input.key_pressed(egui::Key::Escape) {
                eprintln!("Escape pressed");
                ctx.data_mut(|d| d.remove_temp::<SelectionState>(egui::Id::new("selection")));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            ctx.data_mut(|d| d.insert_temp(egui::Id::new("selection"), state));
        }
    }

    let app = OverlayApp {
        result: result_clone,
        screens: screens_clone,
        min_x,
        min_y,
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_fullscreen(true)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_visible(true),
        ..Default::default()
    };

    eprintln!("Starting overlay with run_native...");

    let _ = eframe::run_native("QR Selection", native_options, Box::new(|_cc| Ok(Box::new(app))));

    eprintln!("Overlay window closed");

    let final_result = match result.lock().unwrap().take() {
        Some(r) => r,
        None => OverlayResult::Cancelled,
    };
    final_result
}
