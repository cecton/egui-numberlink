//! The egui widget that renders and drives a [`NumberlinkGame`].

use egui::{
    emath::GuiRounding, Color32, Pos2, Rect, Response, Sense, Stroke, TextStyle, Ui, Vec2, Widget,
};

use crate::game::{GameStatus, NumberlinkGame};

/// A colorblind-safe default palette (Okabe-Ito), cycled if the puzzle has
/// more pairs than colors. Overridden entirely via [`NumberlinkWidget::colors`].
pub const DEFAULT_COLORS: &[Color32] = &[
    Color32::from_rgb(0x00, 0x72, 0xB2), // blue
    Color32::from_rgb(0xE6, 0x9F, 0x00), // orange
    Color32::from_rgb(0x00, 0x9E, 0x73), // bluish green
    Color32::from_rgb(0xD5, 0x5E, 0x00), // vermillion
    Color32::from_rgb(0xCC, 0x79, 0xA7), // reddish purple
    Color32::from_rgb(0xF0, 0xE4, 0x42), // yellow
    Color32::from_rgb(0x56, 0xB4, 0xE9), // sky blue
    Color32::from_rgb(0x66, 0x66, 0x66), // grey
];

/// The total footprint [`NumberlinkWidget`] will occupy for `game` at a
/// given `cell_size`. Lets a caller pre-size a container before laying the
/// widget out.
pub fn content_size(game: &NumberlinkGame, cell_size: f32) -> Vec2 {
    Vec2::new(game.width as f32, game.height as f32) * cell_size
}

/// An egui widget that renders an interactive Numberlink board.
///
/// Press and drag from a number's endpoint (or an already-drawn cell of its
/// path) to draw/redraw that number's path towards the other endpoint.
/// Dragging back over the path's own previous cell retracts it by one;
/// dragging onto a different number's path is rejected.
///
/// ```ignore
/// ui.add(egui_numberlink::NumberlinkWidget::new(&mut game));
/// ```
pub struct NumberlinkWidget<'a> {
    game: &'a mut NumberlinkGame,
    cell_size: Option<f32>,
    colors: &'a [Color32],
    show_numbers: bool,
    win_message: Option<String>,
    interactive: bool,
}

impl<'a> NumberlinkWidget<'a> {
    pub fn new(game: &'a mut NumberlinkGame) -> Self {
        Self {
            game,
            cell_size: None,
            colors: DEFAULT_COLORS,
            show_numbers: true,
            win_message: None,
            interactive: true,
        }
    }

    /// Override the size (in logical pixels) of each grid cell. When not
    /// set, the cell size is computed automatically to fill the available
    /// space of the parent container.
    pub fn cell_size(mut self, size: f32) -> Self {
        self.cell_size = Some(size);
        self
    }

    /// Per-number color, indexed by number (`colors[number % colors.len()]`)
    /// — the customization point for embedding apps that want to reskin the
    /// board. Defaults to [`DEFAULT_COLORS`], a colorblind-safe palette.
    ///
    /// # Panics
    ///
    /// Panics if `colors` is empty.
    pub fn colors(mut self, colors: &'a [Color32]) -> Self {
        assert!(!colors.is_empty(), "colors must not be empty");
        self.colors = colors;
        self
    }

    /// Whether each number's digit is drawn on its two endpoint markers.
    /// Defaults to `true` — this is what keeps the board reading as
    /// Numberlink (numbers are the primary identifier) rather than a
    /// colors-only clone; color is a skin on top, not a replacement.
    pub fn show_numbers(mut self, show: bool) -> Self {
        self.show_numbers = show;
        self
    }

    /// Message shown in the win banner drawn over the board once
    /// [`GameStatus::Won`] is reached. Defaults to `"Solved!"`.
    pub fn win_message(mut self, message: impl Into<String>) -> Self {
        self.win_message = Some(message.into());
        self
    }

    /// Whether the widget responds to drags at all. Set to `false` to
    /// render the board read-only. Defaults to `true`.
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }
}

/// White or black, whichever contrasts more with `bg`, for legible text on
/// an arbitrary caller-supplied color.
fn contrasting_text_color(bg: Color32) -> Color32 {
    let luminance =
        0.2126 * f32::from(bg.r()) + 0.7152 * f32::from(bg.g()) + 0.0722 * f32::from(bg.b());
    if luminance > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

impl Widget for NumberlinkWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            game,
            cell_size,
            colors,
            show_numbers,
            win_message,
            interactive,
        } = self;
        let width = game.width;
        let height = game.height;
        let color_for = |number: usize| colors[number % colors.len()];

        let cell_size = cell_size.unwrap_or_else(|| {
            let available = ui.available_size();
            let by_width = available.x / width as f32;
            let by_height = available.y / height as f32;
            by_width.min(by_height).max(4.0)
        });

        let total_size = Vec2::new(width as f32, height as f32) * cell_size;
        let sense = if interactive {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let (response, painter) = ui.allocate_painter(total_size, sense);
        let origin = response.rect.min;

        let cell_rect = |x: usize, y: usize| -> Rect {
            Rect::from_min_size(
                origin + Vec2::new(x as f32, y as f32) * cell_size,
                Vec2::splat(cell_size),
            )
        };
        let cell_at = |pos: Pos2| -> Option<(usize, usize)> {
            let local = pos - origin;
            if local.x >= 0.0 && local.y >= 0.0 && local.x < total_size.x && local.y < total_size.y
            {
                let cx = (local.x / cell_size).floor() as usize;
                let cy = (local.y / cell_size).floor() as usize;
                if cx < width && cy < height {
                    return Some((cx, cy));
                }
            }
            None
        };

        // ── Input handling ──────────────────────────────────────────────
        // The number being drawn is decided once, from the cell the drag
        // started on, and stashed in egui's per-widget temp memory so every
        // subsequent frame of the same gesture keeps extending it.
        let drag_id = response.id.with("numberlink_drag_number");
        if response.drag_started() {
            if let Some(cell) = response.interact_pointer_pos().and_then(cell_at) {
                if let Some(number) = game.number_at(cell) {
                    if game.start_drag(number, cell) {
                        ui.ctx().data_mut(|d| d.insert_temp(drag_id, number));
                    }
                }
            }
        } else if response.dragged() {
            if let Some(number) = ui.ctx().data(|d| d.get_temp::<usize>(drag_id)) {
                if let Some(cell) = response.interact_pointer_pos().and_then(cell_at) {
                    game.drag_to(number, cell);
                }
            }
        }
        if response.drag_stopped() {
            if let Some(number) = ui.ctx().data_mut(|d| d.remove_temp::<usize>(drag_id)) {
                game.end_drag(number);
            }
        }

        // ── Painting ─────────────────────────────────────────────────────
        let visuals = ui.visuals();
        let ppi = painter.ctx().pixels_per_point();

        // Board background: plain bordered cells.
        for y in 0..height {
            for x in 0..width {
                let rect = cell_rect(x, y).round_to_pixels(ppi);
                painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(0.5, visuals.widgets.noninteractive.bg_stroke.color),
                    egui::StrokeKind::Inside,
                );
            }
        }

        // Paths: drawn through cell *centers*, turning at a cell's center —
        // never along a cell's border/edge — so the pipe visibly runs
        // through the interior of every cell it occupies.
        let pipe_width = cell_size * 0.36;
        let pipe_radius = pipe_width * 0.5;
        for number in 0..game.pair_count() {
            let path = game.path_cells(number);
            if path.len() < 2 {
                continue;
            }
            let color = color_for(number);
            let stroke = Stroke::new(pipe_width, color);
            for pair in path.windows(2) {
                let a = cell_rect(pair[0].0, pair[0].1).center();
                let b = cell_rect(pair[1].0, pair[1].1).center();
                painter.line_segment([a, b], stroke);
            }
            // Rounded joints: a filled circle at every interior cell of the
            // path (the two ends get their own bigger endpoint marker below).
            for &(x, y) in &path[1..path.len() - 1] {
                painter.circle_filled(cell_rect(x, y).center(), pipe_radius, color);
            }
        }

        // Endpoint markers: always drawn (regardless of path state) so the
        // puzzle's numbers are visible before the player starts, and drawn
        // last so they sit cleanly on top of a path's end.
        let marker_radius = cell_size * 0.32;
        let font = TextStyle::Button.resolve(ui.style());
        for number in 0..game.pair_count() {
            let color = color_for(number);
            let text_color = contrasting_text_color(color);
            let (a, b) = game.endpoints(number);
            for &(x, y) in &[a, b] {
                let center = cell_rect(x, y).center();
                painter.circle_filled(center, marker_radius, color);
                if show_numbers {
                    painter.text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        (number + 1).to_string(),
                        font.clone(),
                        text_color,
                    );
                }
            }
        }

        // Win banner, drawn last so it sits on top of everything else.
        if game.status == GameStatus::Won {
            let board_rect = Rect::from_min_size(origin, total_size);
            let banner_size = Vec2::new(
                (total_size.x - 20.0).max(20.0),
                64.0_f32.min(total_size.y - 4.0).max(20.0),
            );
            let banner = Rect::from_center_size(board_rect.center(), banner_size);
            painter.rect_filled(banner, 8.0, Color32::from_black_alpha(170));
            painter.text(
                banner.center(),
                egui::Align2::CENTER_CENTER,
                win_message.as_deref().unwrap_or("Solved!"),
                TextStyle::Heading.resolve(ui.style()),
                Color32::from_rgb(170, 240, 170),
            );
        }

        response
    }
}
