// The #[run_example] macro generates:
//   - wasm32: a #[wasm_bindgen(start)] that calls this function body
//   - native: a main with `dist` / `start` sub-commands that build the wasm
//             bundle and serve it via a local dev server
#[xtask_wasm::run_example(assets_dir = "assets")]
fn run() {
    use eframe::egui;
    use egui_numberlink::{content_size, GameStatus, NumberlinkGame, NumberlinkWidget};
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

    /// The mobile action bar's mode: either the board is pannable/zoomable,
    /// or it's fixed and a drag draws a path. Only one is active at a time,
    /// since both want to own the same drag gesture.
    #[derive(Clone, Copy, PartialEq)]
    enum MobileMode {
        Pan,
        Draw,
    }

    struct NumberlinkApp {
        game: NumberlinkGame,
        selected_preset: Preset,
        seed_counter: u64,
        mobile_mode: MobileMode,
        scene_rect: Option<egui::Rect>,
        show_menu: bool,
        touch_device: bool,
    }

    impl eframe::App for NumberlinkApp {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let bg = ui.max_rect();
            ui.painter()
                .rect_filled(bg, egui::CornerRadius::ZERO, ui.visuals().panel_fill);

            let is_mobile = self.is_mobile(ui);

            self.show_top_bar(ui, is_mobile);

            if is_mobile {
                self.mobile_ui(ui);
                self.show_menu_modal(ui.ctx());
            } else {
                self.desktop_ui(ui);
                self.show_menu = false;
            }
        }

        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            eframe::set_value(storage, SELECTED_PRESET_KEY, &self.selected_preset);
        }
    }

    impl NumberlinkApp {
        const MOBILE_CELL_SIZE: f32 = 40.0;
        const ACTION_BUTTON_SIZE: egui::Vec2 = egui::vec2(48.0, 48.0);

        fn new_game(&mut self, preset: Preset) {
            self.selected_preset = preset;
            let (w, h, pairs) = preset.dims();
            self.seed_counter += 1;
            self.game = NumberlinkGame::random(w, h, pairs, self.seed_counter);
            self.scene_rect = None;
            self.mobile_mode = MobileMode::Draw;
        }

        fn start_new_game(&mut self) {
            let preset = self.selected_preset;
            self.new_game(preset);
        }

        fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let selected_preset = cc
                .storage
                .and_then(|storage| eframe::get_value(storage, SELECTED_PRESET_KEY))
                .unwrap_or(Preset::Beginner);
            let (w, h, pairs) = selected_preset.dims();
            let initial_seed = fastrand::u64(..);

            let touch_device = web_sys::window()
                .and_then(|w| w.match_media("(pointer: coarse)").ok())
                .flatten()
                .is_some_and(|mql| mql.matches());

            Self {
                game: NumberlinkGame::random(w, h, pairs, initial_seed),
                selected_preset,
                seed_counter: initial_seed,
                mobile_mode: MobileMode::Draw,
                scene_rect: None,
                show_menu: false,
                touch_device,
            }
        }

        /// Narrow viewport or a coarse (touch) pointer: switches the app to
        /// the panning, toolbar-driven mobile layout instead of the
        /// fills-the-window desktop one. Pointer coarseness is queried once
        /// at startup (a wasm/JS FFI call) and cached, since it can't
        /// realistically change mid-session and re-checking it every frame
        /// would be wasted work.
        fn is_mobile(&self, ui: &egui::Ui) -> bool {
            let content = ui.ctx().content_rect();
            content.width() < 900.0 || self.touch_device
        }

        fn show_top_bar(&mut self, ui: &mut egui::Ui, is_mobile: bool) {
            if is_mobile {
                // Top bar hidden on mobile; its actions live in the
                // bottom action bar and hamburger menu instead.
                return;
            }

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
                                self.start_new_game();
                            }
                        });
                    });
                });
        }

        fn desktop_ui(&mut self, ui: &mut egui::Ui) {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add(NumberlinkWidget::new(&mut self.game));
            });
        }

        fn mobile_ui(&mut self, ui: &mut egui::Ui) {
            ui.spacing_mut().interact_size.y = 48.0;

            self.show_action_bar(ui);
            let content_area = ui.available_rect_before_wrap();

            let board_footprint = content_size(&self.game, Self::MOBILE_CELL_SIZE);
            let mut scene_rect = self
                .scene_rect
                .unwrap_or_else(|| egui::Rect::from_min_size(egui::Pos2::ZERO, board_footprint));
            let zoom_range = egui::Rangef::new(0.25, 4.0);

            if self.mobile_mode == MobileMode::Pan {
                // Panning: the widget is disabled so it never claims the
                // drag, leaving Scene's own background free to pan/zoom.
                egui::containers::Scene::new()
                    .zoom_range(zoom_range)
                    .max_inner_size(board_footprint)
                    .show(ui, &mut scene_rect, |ui| {
                        ui.add(
                            NumberlinkWidget::new(&mut self.game)
                                .cell_size(Self::MOBILE_CELL_SIZE)
                                .interactive(false),
                        );
                    });
            } else {
                // Draw: the widget stays fully interactive to draw paths,
                // and the view is rendered locked (no Scene pan/zoom
                // registration at all) so no gesture can move it.
                Self::show_locked_scene(ui, scene_rect, board_footprint, zoom_range, |ui| {
                    ui.add(NumberlinkWidget::new(&mut self.game).cell_size(Self::MOBILE_CELL_SIZE));
                });
            }

            self.scene_rect = Some(scene_rect);

            if self.game.status == GameStatus::Won {
                egui::Area::new(egui::Id::new("mobile_new_game_overlay"))
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 40.0))
                    .constrain_to(content_area)
                    .order(egui::Order::Foreground)
                    .show(ui.ctx(), |ui| {
                        let button =
                            egui::Button::new(egui::RichText::new("\u{1F504} New Game").size(20.0))
                                .min_size(egui::vec2(180.0, 48.0));
                        if ui.add(button).clicked() {
                            self.start_new_game();
                        }
                    });
            }
        }

        /// Renders `add_contents` at `scene_rect`'s pan/zoom transform
        /// without registering `egui::containers::Scene`'s own drag/
        /// scroll/pinch handling (which `Scene::show` always reads
        /// regardless of its `sense` setting). Used whenever Pan mode
        /// isn't active, so the view can't move no matter the gesture.
        fn show_locked_scene(
            ui: &mut egui::Ui,
            scene_rect: egui::Rect,
            max_inner_size: egui::Vec2,
            zoom_range: egui::Rangef,
            add_contents: impl FnOnce(&mut egui::Ui),
        ) {
            let (outer_rect, _) =
                ui.allocate_exact_size(ui.available_size_before_wrap(), egui::Sense::hover());

            // Mirrors `Scene::show`'s own self-healing: a degenerate
            // `scene_rect` (e.g. non-finite or zero-size) would otherwise
            // send `scale` to NaN/inf with no recovery until `new_game()`.
            let scene_rect = if scene_rect.is_finite() && scene_rect.size() != egui::Vec2::ZERO {
                scene_rect
            } else {
                egui::Rect::from_min_size(egui::Pos2::ZERO, max_inner_size)
            };

            let scale = zoom_range.clamp((outer_rect.size() / scene_rect.size()).min_elem());
            let to_global = egui::emath::TSTransform::from_translation(
                outer_rect.center().to_vec2() - scale * scene_rect.center().to_vec2(),
            ) * egui::emath::TSTransform::from_scaling(scale);

            let layer_id = egui::LayerId::new(ui.layer_id().order, ui.id().with("locked_scene"));
            ui.ctx().set_sublayer(ui.layer_id(), layer_id);

            let mut local_ui = ui.new_child(
                egui::UiBuilder::new()
                    .layer_id(layer_id)
                    .max_rect(egui::Rect::from_min_size(egui::Pos2::ZERO, max_inner_size))
                    .sense(egui::Sense::hover()),
            );
            local_ui.set_clip_rect(to_global.inverse() * outer_rect);
            local_ui.ctx().set_transform_layer(layer_id, to_global);

            add_contents(&mut local_ui);
        }

        /// Bottom toolbar for mobile: a hamburger menu for preset/new-game/
        /// theme, a Pan/Draw mode toggle (whichever is active is
        /// highlighted, since a drag gesture can't both pan the scene and
        /// draw a path at once), and Undo/Redo.
        fn show_action_bar(&mut self, ui: &mut egui::Ui) {
            egui::Panel::bottom("action_bar")
                .resizable(false)
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 4)))
                .show(ui, |ui| {
                    let center = egui::Layout::top_down(egui::Align::Center)
                        .with_cross_align(egui::Align::Center);

                    ui.columns(5, |columns| {
                        columns[0].with_layout(center, |ui| {
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new("☰").size(22.0))
                                        .min_size(Self::ACTION_BUTTON_SIZE),
                                )
                                .clicked()
                            {
                                self.show_menu = true;
                            }
                        });

                        columns[1].with_layout(center, |ui| {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        self.mobile_mode == MobileMode::Pan,
                                        egui::RichText::new("\u{1F50D}").size(22.0),
                                    )
                                    .min_size(Self::ACTION_BUTTON_SIZE),
                                )
                                .clicked()
                            {
                                self.mobile_mode = MobileMode::Pan;
                            }
                        });

                        columns[2].with_layout(center, |ui| {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        self.mobile_mode == MobileMode::Draw,
                                        egui::RichText::new("\u{270F}").size(22.0),
                                    )
                                    .min_size(Self::ACTION_BUTTON_SIZE),
                                )
                                .clicked()
                            {
                                self.mobile_mode = MobileMode::Draw;
                            }
                        });

                        columns[3].with_layout(center, |ui| {
                            if ui
                                .add_enabled(
                                    self.game.can_undo(),
                                    egui::Button::new(egui::RichText::new("\u{27F2}").size(22.0))
                                        .min_size(Self::ACTION_BUTTON_SIZE),
                                )
                                .clicked()
                            {
                                self.game.undo();
                            }
                        });

                        columns[4].with_layout(center, |ui| {
                            if ui
                                .add_enabled(
                                    self.game.can_redo(),
                                    egui::Button::new(egui::RichText::new("\u{27F3}").size(22.0))
                                        .min_size(Self::ACTION_BUTTON_SIZE),
                                )
                                .clicked()
                            {
                                self.game.redo();
                            }
                        });
                    });
                });
        }

        fn show_menu_modal(&mut self, ctx: &egui::Context) {
            if !self.show_menu {
                return;
            }

            let vp_width = ctx.viewport_rect().width();
            let area = egui::Modal::default_area(egui::Id::new("menu_modal"))
                .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::ZERO)
                .default_width(vp_width);

            let menu_font_size = 24.0;
            let response = egui::Modal::new(egui::Id::new("menu_modal"))
                .area(area)
                .frame(
                    egui::Frame::popup(&ctx.global_style())
                        .inner_margin(egui::Margin::symmetric(16, 16)),
                )
                .backdrop_color(egui::Color32::from_black_alpha(128))
                .show(ctx, |ui| {
                    ui.set_min_width((vp_width - 32.0).max(0.0));
                    ui.spacing_mut().interact_size.y = 36.0;
                    {
                        let prev = ui.visuals().button_frame;
                        ui.visuals_mut().button_frame = false;
                        if ui
                            .button(egui::RichText::new("🔄 New Game").size(menu_font_size))
                            .clicked()
                        {
                            self.start_new_game();
                            self.show_menu = false;
                        }
                        ui.visuals_mut().button_frame = prev;
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Difficulty").size(menu_font_size));
                    for &preset in Preset::ALL {
                        if ui
                            .selectable_label(
                                self.selected_preset == preset,
                                egui::RichText::new(preset.label()).size(menu_font_size),
                            )
                            .clicked()
                        {
                            self.new_game(preset);
                            self.show_menu = false;
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Theme").size(menu_font_size));
                    egui::widgets::global_theme_preference_buttons(ui);
                });

            if response.should_close() {
                self.show_menu = false;
            }
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
