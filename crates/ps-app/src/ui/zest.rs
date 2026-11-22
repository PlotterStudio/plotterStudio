//! Drawing helpers for the Zest design: the orange gradient, line icons, weighted text and the
//! small custom controls (segmented switch, tiny buttons, badges) the panels share.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, vec2};

use crate::theme::Palette;

/// The palette of the context's current theme.
pub(super) fn palette(ctx: &egui::Context) -> Palette {
    Palette::of(ctx.theme())
}

/// Font weights the design uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Weight {
    Regular,
    Medium,
    Semibold,
}

/// The font family of a weight; the heavier faces fall back to Regular until they are loaded.
pub(super) fn family(ctx: &egui::Context, weight: Weight) -> FontFamily {
    let name = match weight {
        Weight::Regular => return FontFamily::Proportional,
        Weight::Medium => "medium",
        Weight::Semibold => "semibold",
    };
    let family = FontFamily::Name(name.into());
    if ctx.fonts(|f| f.families().contains(&family)) {
        family
    } else {
        FontFamily::Proportional
    }
}

/// Text of `size` points in a weight and colour.
pub(super) fn text(
    ctx: &egui::Context,
    s: impl Into<String>,
    size: f32,
    weight: Weight,
    colour: Color32,
) -> RichText {
    RichText::new(s)
        .size(size)
        .family(family(ctx, weight))
        .color(colour)
}

/// A font id of a weight, for painter text.
pub(super) fn font(ctx: &egui::Context, size: f32, weight: Weight) -> FontId {
    FontId::new(size, family(ctx, weight))
}

/// The three stops of the accent gradient: amber to orange.
const STOPS: [(f32, Color32); 3] = [
    (0.0, Color32::from_rgb(0xff, 0xc2, 0x3d)),
    (0.55, Color32::from_rgb(0xff, 0x8a, 0x1f)),
    (1.0, Color32::from_rgb(0xff, 0x7a, 0x1a)),
];

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| {
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an interpolation between two bytes stays in 0..=255"
        )]
        let byte = v.round().clamp(0.0, 255.0) as u8;
        byte
    };
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// The accent gradient at `t` in 0..=1.
pub(super) fn accent_at(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    for pair in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
        if t <= t1 {
            return lerp(c0, c1, (t - t0) / (t1 - t0));
        }
    }
    STOPS[2].1
}

/// Paints the 135 degree accent gradient into a rounded rectangle.
pub(super) fn gradient_rect(painter: &egui::Painter, rect: Rect, radius: f32) {
    // A mesh has no anti-aliased edge, so a solid rounded shape of the middle colour goes under
    // it and the gradient stops half a pixel short: the outline is smooth, the fill graded.
    painter.rect_filled(rect, radius, accent_at(0.55));
    let rect = rect.shrink(0.75);
    let radius = (radius - 0.75).max(0.0);
    let (w, h) = (rect.width().max(1.0), rect.height().max(1.0));
    let radius = radius.min(h / 2.0).min(w / 2.0);
    let mut mesh = Mesh::default();
    let step = 1.0_f32;
    let mut x = 0.0_f32;
    while x < w {
        let x1 = (x + step).min(w);
        let inset = |dx: f32| {
            let d = if dx < radius {
                radius - dx
            } else if dx > w - radius {
                dx - (w - radius)
            } else {
                0.0
            };
            if d <= 0.0 {
                0.0
            } else {
                radius - (radius * radius - d * d).max(0.0).sqrt()
            }
        };
        let mid = f32::midpoint(x, x1);
        let inset = inset(mid);
        let (top, bottom) = (rect.top() + inset, rect.bottom() - inset);
        let t = |px: f32, py: f32| (px + py) / (w + h);
        let base = u32::try_from(mesh.vertices.len()).unwrap_or(0);
        for (px, py, y) in [
            (x, inset, top),
            (x1, inset, top),
            (x1, h - inset, bottom),
            (x, h - inset, bottom),
        ] {
            mesh.vertices.push(Vertex {
                pos: Pos2::new(rect.left() + px, y),
                uv: WHITE_UV,
                color: accent_at(t(px, py)),
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        x = x1;
    }
    painter.add(Shape::mesh(mesh));
}

/// A soft drop shadow below a rounded rectangle.
pub(super) fn shadow(painter: &egui::Painter, rect: Rect, radius: u8, strong: bool) {
    let (offset, blur, alpha) = if strong { (10, 30, 50) } else { (4, 16, 14) };
    painter.add(
        egui::epaint::Shadow {
            offset: [0, offset],
            blur,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, alpha),
        }
        .as_shape(rect, egui::CornerRadius::same(radius)),
    );
}

/// The line icons of the design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Icon {
    Plus,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Eye,
    EyeOff,
    Book,
    Link,
    Trash,
    Close,
    Search,
    Warning,
    Send,
    Pause,
    Play,
}

/// Draws `icon` centred on `c`, `size` points across, with a stroke of `width`.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per icon, each a few line segments"
)]
pub(super) fn icon(painter: &egui::Painter, c: Pos2, icon: Icon, size: f32, colour: Color32) {
    let u = size / 24.0;
    let s = Stroke::new((1.75 * u * 24.0 / size.max(1.0)).max(1.25), colour);
    let at = |x: f32, y: f32| Pos2::new(c.x + (x - 12.0) * u, c.y + (y - 12.0) * u);
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(pts.iter().map(|p| at(p.0, p.1)).collect(), s));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(
            pts.iter().map(|p| at(p.0, p.1)).collect(),
            s,
        ));
    };
    match icon {
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::ChevronDown => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Icon::ChevronRight => line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
        Icon::ChevronLeft => line(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]),
        Icon::Eye => {
            closed(&[
                (2.0, 12.0),
                (6.0, 7.0),
                (12.0, 5.0),
                (18.0, 7.0),
                (22.0, 12.0),
                (18.0, 17.0),
                (12.0, 19.0),
                (6.0, 17.0),
            ]);
            painter.circle_stroke(at(12.0, 12.0), 3.0 * u, s);
        }
        Icon::EyeOff => {
            closed(&[
                (2.0, 12.0),
                (6.0, 7.0),
                (12.0, 5.0),
                (18.0, 7.0),
                (22.0, 12.0),
                (18.0, 17.0),
                (12.0, 19.0),
                (6.0, 17.0),
            ]);
            line(&[(3.0, 3.0), (21.0, 21.0)]);
        }
        Icon::Book => {
            line(&[
                (4.0, 4.0),
                (10.0, 4.0),
                (12.0, 6.0),
                (12.0, 20.0),
                (10.0, 18.0),
                (4.0, 18.0),
                (4.0, 4.0),
            ]);
            line(&[
                (20.0, 4.0),
                (14.0, 4.0),
                (12.0, 6.0),
                (12.0, 20.0),
                (14.0, 18.0),
                (20.0, 18.0),
                (20.0, 4.0),
            ]);
        }
        Icon::Link => {
            line(&[(10.0, 14.0), (14.0, 10.0)]);
            line(&[
                (8.0, 12.0),
                (5.0, 15.0),
                (5.0, 19.0),
                (9.0, 19.0),
                (12.0, 16.0),
            ]);
            line(&[
                (16.0, 12.0),
                (19.0, 9.0),
                (19.0, 5.0),
                (15.0, 5.0),
                (12.0, 8.0),
            ]);
        }
        Icon::Trash => {
            line(&[(4.0, 7.0), (20.0, 7.0)]);
            line(&[(9.0, 7.0), (9.0, 4.0), (15.0, 4.0), (15.0, 7.0)]);
            line(&[(6.0, 7.0), (7.0, 20.0), (17.0, 20.0), (18.0, 7.0)]);
            line(&[(10.0, 11.0), (10.0, 16.0)]);
            line(&[(14.0, 11.0), (14.0, 16.0)]);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Search => {
            painter.circle_stroke(at(11.0, 11.0), 7.0 * u, s);
            line(&[(21.0, 21.0), (16.7, 16.7)]);
        }
        Icon::Warning => {
            closed(&[(12.0, 3.0), (22.0, 20.0), (2.0, 20.0)]);
            line(&[(12.0, 10.0), (12.0, 14.0)]);
            line(&[(12.0, 17.0), (12.0, 17.2)]);
        }
        Icon::Send => {
            closed(&[(21.0, 3.0), (3.0, 10.0), (10.0, 13.0), (13.0, 21.0)]);
            line(&[(21.0, 3.0), (10.0, 13.0)]);
        }
        Icon::Pause => {
            line(&[(8.0, 5.0), (8.0, 19.0)]);
            line(&[(16.0, 5.0), (16.0, 19.0)]);
        }
        Icon::Play => closed(&[(7.0, 4.0), (20.0, 12.0), (7.0, 20.0)]),
    }
}

/// The logo mark: a rounded gradient tile with a pen nib.
pub(super) fn logo(ui: &mut egui::Ui, size: f32, ink: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    gradient_rect(ui.painter(), rect, size * 0.3);
    let c = rect.center();
    let u = size * 0.55 / 24.0;
    let s = Stroke::new(1.5_f32, ink);
    let at = |x: f32, y: f32| Pos2::new(c.x + (x - 12.0) * u, c.y + (y - 12.0) * u);
    let pts = |v: &[(f32, f32)]| v.iter().map(|p| at(p.0, p.1)).collect::<Vec<_>>();
    ui.painter().add(Shape::closed_line(
        pts(&[(12.0, 19.0), (19.0, 12.0), (22.0, 15.0), (15.0, 22.0)]),
        s,
    ));
    ui.painter().add(Shape::closed_line(
        pts(&[
            (18.0, 13.0),
            (16.5, 5.5),
            (2.0, 2.0),
            (5.5, 16.5),
            (13.0, 18.0),
        ]),
        s,
    ));
    ui.painter()
        .add(Shape::line(pts(&[(2.0, 2.0), (9.6, 9.6)]), s));
}

/// Draws a line swatch for an operation: solid, dashed or wavy, `width` points across.
pub(super) fn line_swatch(
    painter: &egui::Painter,
    from: Pos2,
    width: f32,
    colour: Color32,
    style: ps_model::LineStyle,
    thickness: f32,
) {
    let stroke = Stroke::new(thickness, colour);
    match style {
        ps_model::LineStyle::Dashed => {
            let mut x = 0.0;
            while x < width {
                let end = (x + 4.0).min(width);
                painter.line_segment(
                    [
                        Pos2::new(from.x + x, from.y),
                        Pos2::new(from.x + end, from.y),
                    ],
                    stroke,
                );
                x += 7.0;
            }
        }
        ps_model::LineStyle::Dotted => {
            let mut x = 0.5;
            while x < width {
                painter.circle_filled(Pos2::new(from.x + x, from.y), thickness / 2.0, colour);
                x += 3.5;
            }
        }
        ps_model::LineStyle::Wavy => {
            let pts: Vec<Pos2> = (0..=16_u8)
                .map(|i| {
                    let t = f32::from(i) / 16.0;
                    Pos2::new(
                        from.x + t * width,
                        from.y - (t * std::f32::consts::TAU * 2.0).sin() * 2.0,
                    )
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
        }
        _ => {
            painter.line_segment([from, Pos2::new(from.x + width, from.y)], stroke);
        }
    }
}

/// A small status badge ("Preset", "Changed"): `fill` behind `ink` text at 10 points.
pub(super) fn badge(ui: &mut egui::Ui, label: &str, fill: Color32, ink: Color32) {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        font(ui.ctx(), 10.0, Weight::Semibold),
        ink,
    );
    let size = vec2(galley.size().x + 8.0, 14.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter().galley(
        Pos2::new(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.to_owned()));
}

/// A 1 point hairline across `rect`'s width or down its height.
pub(super) fn hairline(painter: &egui::Painter, a: Pos2, b: Pos2, colour: Color32) {
    painter.line_segment([a, b], Stroke::new(1.0_f32, colour));
}

/// Like [`icon_button`] but can be disabled: it then neither reacts nor lets a popup open.
pub(super) fn icon_button_enabled(
    ui: &mut egui::Ui,
    pal: Palette,
    glyph: Icon,
    size: egui::Vec2,
    icon_size: f32,
    enabled: bool,
    label: &str,
) -> egui::Response {
    if enabled {
        return icon_button(ui, pal, glyph, size, icon_size, false, label);
    }
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    icon(ui.painter(), rect.center(), glyph, icon_size, pal.outline);
    let name = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, false, name.clone())
    });
    response
}

/// An icon-only button of `size` points: tinted when `on`, hover fill otherwise. The label is for
/// screen readers and the tooltip.
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    pal: Palette,
    glyph: Icon,
    size: egui::Vec2,
    icon_size: f32,
    on: bool,
    label: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = 3.0;
    if on {
        ui.painter().rect_filled(rect, radius, pal.tint);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, pal.hover);
    }
    let colour = if on { pal.accent } else { pal.muted };
    icon(ui.painter(), rect.center(), glyph, icon_size, colour);
    let name = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name.clone())
    });
    response.on_hover_text(label)
}
