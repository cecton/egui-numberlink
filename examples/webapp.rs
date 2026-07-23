// The #[run_example] macro generates:
//   - wasm32: a #[wasm_bindgen(start)] that calls this function body
//   - native: a main with `dist` / `start` sub-commands that build the wasm
//             bundle and serve it via a local dev server
#[xtask_wasm::run_example(assets_dir = "assets")]
fn run() {
    use eframe::egui;
    use egui_numberlink::{GameStatus, NumberlinkGame, NumberlinkWidget};
    use serde::{Deserialize, Serialize};
    use xtask_wasm::wasm_bindgen::JsCast as _;

    const SELECTED_PRESET_KEY: &str = "selected_preset";

    #[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
    enum Preset {
        Beginner,
        Intermediate,
        Expert,
    }

    impl Preset {
        const ALL: &'static [Preset] = &[Self::Beginner, Self::Intermediate, Self::Expert];

        fn label(self) -> &'static str {
            match self {
                Self::Beginner => "Beginner (5x5, 4 pairs)",
                Self::Intermediate => "Intermediate (7x7, 6 pairs)",
                Self::Expert => "Expert (9x9, 8 pairs)",
            }
        }

        fn dims(self) -> (usize, usize, usize) {
            match self {
                Self::Beginner => (5, 5, 4),
                Self::Intermediate => (7, 7, 6),
                Self::Expert => (9, 9, 8),
            }
        }
    }

    struct NumberlinkApp {
        game: NumberlinkGame,
        selected_preset: Preset,
        seed_counter: u64,
    }

    impl eframe::App for NumberlinkApp {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let bg = ui.max_rect();
            ui.painter()
                .rect_filled(bg, egui::CornerRadius::ZERO, ui.visuals().panel_fill);

            self.show_top_bar(ui);

            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add(NumberlinkWidget::new(&mut self.game));
            });
        }

        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            eframe::set_value(storage, SELECTED_PRESET_KEY, &self.selected_preset);
        }
    }

    impl NumberlinkApp {
        fn new_game(&mut self, preset: Preset) {
            self.selected_preset = preset;
            let (w, h, pairs) = preset.dims();
            self.seed_counter += 1;
            self.game = NumberlinkGame::random(w, h, pairs, self.seed_counter);
        }

        fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let selected_preset = cc
                .storage
                .and_then(|storage| eframe::get_value(storage, SELECTED_PRESET_KEY))
                .unwrap_or(Preset::Beginner);
            let (w, h, pairs) = selected_preset.dims();
            let initial_seed = fastrand::u64(..);

            Self {
                game: NumberlinkGame::random(w, h, pairs, initial_seed),
                selected_preset,
                seed_counter: initial_seed,
            }
        }

        fn show_top_bar(&mut self, ui: &mut egui::Ui) {
            egui::Panel::top("top_bar")
                .frame(egui::Frame::new().inner_margin(4.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.visuals_mut().button_frame = false;
                        ui.add_space(8.0);
                        egui::widgets::global_theme_preference_switch(ui);
                        ui.separator();
                        for &preset in Preset::ALL {
                            if ui
                                .selectable_label(self.selected_preset == preset, preset.label())
                                .clicked()
                            {
                                self.new_game(preset);
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(self.game.can_undo(), egui::Button::new("\u{27F2} Undo"))
                            .clicked()
                        {
                            self.game.undo();
                        }
                        if ui
                            .add_enabled(self.game.can_redo(), egui::Button::new("\u{27F3} Redo"))
                            .clicked()
                        {
                            self.game.redo();
                        }
                        if self.game.status == GameStatus::Won {
                            ui.colored_label(egui::Color32::GREEN, "Solved!");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("\u{1F504} New Game").clicked() {
                                let preset = self.selected_preset;
                                self.new_game(preset);
                            }
                        });
                    });
                });
        }
    }

    // Create a full-screen canvas and attach it to the page body.
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");

    let canvas = document
        .create_element("canvas")
        .expect("failed to create canvas")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("not a HtmlCanvasElement");

    let style = canvas.style();
    style.set_property("position", "fixed").unwrap();
    style.set_property("top", "0").unwrap();
    style.set_property("left", "0").unwrap();
    style.set_property("width", "100%").unwrap();
    style.set_property("height", "100%").unwrap();

    let body = document.body().expect("no body");
    body.style().set_property("margin", "0").unwrap();
    body.append_child(&canvas).expect("failed to append canvas");
    canvas.style().set_property("touch-action", "none").unwrap();

    // Start the eframe web runner on that canvas element.
    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(NumberlinkApp::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
}
