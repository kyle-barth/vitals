//! A thin layer over Direct2D and DirectWrite. All coordinates are
//! device-independent pixels, so text and marks stay sharp at any display scale.
//!
//! Text is set in Segoe UI Variable, Windows 11's interface font: its Text cut
//! for words and its Display cut for large numbers. Windows 10 lacks it, so
//! both fall back to Segoe UI there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED,
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_CUSTOM,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_OPTIONS,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Brush, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_LINE_METRICS, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{BOOL, HSTRING, Interface, Result, w};
use windows_numerics::Vector2;

use crate::apps::Icon;
use crate::layout::Rect;

/// Segoe UI's ascent, as a fraction of the font size, for a format whose own
/// baseline can't be read.
const ASCENT: f32 = 1.079;
/// The cap height of both faces, as a fraction of the font size.
const CAP_HEIGHT: f32 = 0.70;
const FAR: f32 = 16_384.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Color {
    pub const fn rgb(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xff) as f32 / 255.0,
            g: ((hex >> 8) & 0xff) as f32 / 255.0,
            b: (hex & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
}

/// Which cut of the font: Text for words, Display for large numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Text,
    Display,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// Text formats are cached by half-pixel size, which bounds the cache however
/// much the window is resized.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FormatKey {
    half_points: u16,
    weight: Weight,
    align: Align,
    face: Face,
}

/// A text format and the distance from its top to its baseline.
#[derive(Clone)]
struct Format {
    format: IDWriteTextFormat,
    baseline: f32,
}

struct Target {
    render: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
    /// Bitmaps belong to their render target, so they are dropped along with it.
    icons: RefCell<HashMap<String, ID2D1Bitmap>>,
}

pub struct Gfx {
    hwnd: HWND,
    dpi: f32,
    factory: ID2D1Factory,
    write: IDWriteFactory,
    round: ID2D1StrokeStyle,
    /// Dashed strokes, by their dash and gap lengths in hundredths of a pixel.
    dashes: RefCell<HashMap<(u32, u32), ID2D1StrokeStyle>>,
    formats: RefCell<HashMap<FormatKey, Format>>,
    /// The font families for each face, which are Segoe UI where Segoe UI
    /// Variable isn't installed.
    families: (HSTRING, HSTRING),
    target: Option<Target>,
}

/// The family to use for `wanted`, or Segoe UI when this PC doesn't have it.
fn family(write: &IDWriteFactory, wanted: &str) -> HSTRING {
    let name = HSTRING::from(wanted);
    let mut collection: Option<IDWriteFontCollection> = None;
    let (mut index, mut exists) = (0u32, BOOL(0));
    // SAFETY: the out pointers are valid for each call, and the name outlives it.
    let found = unsafe {
        write
            .GetSystemFontCollection(&mut collection, false)
            .is_ok()
            && collection.is_some_and(|c| {
                c.FindFamilyName(&name, &mut index, &mut exists).is_ok() && exists.as_bool()
            })
    };
    if found {
        name
    } else {
        HSTRING::from("Segoe UI")
    }
}

impl Gfx {
    pub fn new(hwnd: HWND, dpi: f32) -> Result<Self> {
        let options = D2D1_FACTORY_OPTIONS::default();
        // SAFETY: the factories are COM objects released on drop, and the
        // options and stroke properties are valid for each call.
        unsafe {
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options))?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let round = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.0,
                    ..Default::default()
                },
                None,
            )?;
            let families = (
                family(&write, "Segoe UI Variable Text"),
                family(&write, "Segoe UI Variable Display"),
            );
            Ok(Self {
                hwnd,
                dpi,
                factory,
                write,
                round,
                dashes: Default::default(),
                formats: Default::default(),
                families,
                target: None,
            })
        }
    }

    pub fn set_dpi(&mut self, dpi: f32) {
        self.dpi = dpi;
        if let Some(target) = &self.target {
            // SAFETY: the render target is valid while it is held.
            unsafe { target.render.SetDpi(dpi, dpi) };
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(target) = &self.target {
            let size = D2D_SIZE_U { width, height };
            // SAFETY: the render target is valid while it is held.
            if unsafe { target.render.Resize(&size) }.is_err() {
                self.target = None;
            }
        }
    }

    /// Lets go of every icon. The Apps page can hold hundreds of them, and any
    /// still needed are simply made again.
    pub fn drop_icons(&mut self) {
        if let Some(target) = &self.target {
            target.icons.borrow_mut().clear();
        }
    }

    fn ensure_target(&mut self) -> Result<()> {
        if self.target.is_some() {
            return Ok(());
        }
        let mut client = RECT::default();
        // SAFETY: the window outlives `Gfx`, and the out pointers are valid.
        unsafe {
            GetClientRect(self.hwnd, &mut client)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                // Software rendering, measured on this app: the same CPU use as
                // hardware, under half the memory, and no load on the GPU it is
                // monitoring. One redraw per second gains nothing from a GPU.
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: self.dpi,
                dpiY: self.dpi,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let window = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: (client.right - client.left).max(1) as u32,
                    height: (client.bottom - client.top).max(1) as u32,
                },
                // Presenting without waiting for vsync keeps live resizing
                // smooth. At one redraw per second there is nothing to tear.
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };
            let render = self.factory.CreateHwndRenderTarget(&properties, &window)?;
            let brush = render.CreateSolidColorBrush(&Color::rgb(0).d2d(), None)?;
            self.target = Some(Target {
                render,
                brush,
                icons: RefCell::default(),
            });
        }
        Ok(())
    }

    /// Draws one frame. If the GPU device was lost, the target is dropped and
    /// rebuilt on the next frame.
    pub fn draw(&mut self, paint: impl FnOnce(&Canvas)) -> Result<()> {
        self.ensure_target()?;
        let Some(target) = &self.target else {
            return Ok(());
        };
        let canvas = Canvas { gfx: self, target };
        // SAFETY: Begin and End are always paired on a valid target.
        let result = unsafe {
            target.render.BeginDraw();
            paint(&canvas);
            target.render.EndDraw(None, None)
        };
        match result {
            Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
                self.target = None;
                Ok(())
            }
            other => other,
        }
    }
}

/// Drawing operations available during a frame.
pub struct Canvas<'a> {
    gfx: &'a Gfx,
    target: &'a Target,
}

fn d2d_rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

impl Canvas<'_> {
    fn brush(&self, color: Color) -> &ID2D1SolidColorBrush {
        // SAFETY: the brush is valid for the frame, and the color outlives the call.
        unsafe { self.target.brush.SetColor(&color.d2d()) };
        &self.target.brush
    }

    pub fn clear(&self, color: Color) {
        // SAFETY: called between BeginDraw and EndDraw.
        unsafe { self.target.render.Clear(Some(&color.d2d())) };
    }

    pub fn fill_round(&self, r: Rect, radius: f32, color: Color) {
        let shape = D2D1_ROUNDED_RECT {
            rect: d2d_rect(r),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: called between BeginDraw and EndDraw.
        unsafe {
            self.target
                .render
                .FillRoundedRectangle(&shape, self.brush(color))
        };
    }

    pub fn circle(&self, x: f32, y: f32, radius: f32, color: Color) {
        let shape = D2D1_ELLIPSE {
            point: Vector2 { X: x, Y: y },
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: called between BeginDraw and EndDraw.
        unsafe { self.target.render.FillEllipse(&shape, self.brush(color)) };
    }

    pub fn outline_circle(&self, x: f32, y: f32, radius: f32, width: f32, color: Color) {
        let shape = D2D1_ELLIPSE {
            point: Vector2 { X: x, Y: y },
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: called between BeginDraw and EndDraw.
        unsafe {
            self.target
                .render
                .DrawEllipse(&shape, self.brush(color), width, None)
        };
    }

    pub fn outline_rect(&self, r: Rect, width: f32, color: Color) {
        // SAFETY: called between BeginDraw and EndDraw.
        unsafe {
            self.target
                .render
                .DrawRectangle(&d2d_rect(r), self.brush(color), width, None)
        };
    }

    /// Draws an app's icon, keeping it as a bitmap under `key` for later frames.
    pub fn icon(&self, key: &str, icon: &Icon, r: Rect) {
        let mut icons = self.target.icons.borrow_mut();
        if !icons.contains_key(key) {
            let properties = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let size = D2D_SIZE_U {
                width: icon.size,
                height: icon.size,
            };
            // SAFETY: the pixels hold `size` rows of `size * 4` bytes, and
            // Direct2D copies them during the call.
            let created = unsafe {
                self.target.render.CreateBitmap(
                    size,
                    Some(icon.pixels.as_ptr().cast()),
                    icon.size * 4,
                    &properties,
                )
            };
            let Ok(bitmap) = created else { return };
            icons.insert(key.to_string(), bitmap);
        }
        if let Some(bitmap) = icons.get(key) {
            // SAFETY: called between BeginDraw and EndDraw, and the bitmap was
            // created on this target.
            unsafe {
                self.target.render.DrawBitmap(
                    bitmap,
                    Some(&d2d_rect(r)),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
            }
        }
    }

    /// Shortens `text` with an ellipsis until it fits in `width`.
    pub fn fit(&self, text: &str, size: f32, weight: Weight, width: f32) -> String {
        if self.measure(text, size, weight) <= width {
            return text.to_string();
        }
        let mut kept: Vec<char> = text.chars().collect();
        while !kept.is_empty() {
            kept.pop();
            let candidate: String = kept.iter().chain(['…'].iter()).collect();
            if self.measure(&candidate, size, weight) <= width {
                return candidate;
            }
        }
        String::new()
    }

    pub fn line(&self, from: (f32, f32), to: (f32, f32), width: f32, color: Color) {
        let point = |(x, y): (f32, f32)| Vector2 { X: x, Y: y };
        // SAFETY: called between BeginDraw and EndDraw, and the stroke style
        // lives as long as the `Gfx`.
        unsafe {
            self.target.render.DrawLine(
                point(from),
                point(to),
                self.brush(color),
                width,
                &self.gfx.round,
            );
        }
    }

    /// Draws a connected line with round joins and caps.
    pub fn polyline(&self, points: &[(f32, f32)], width: f32, color: Color) {
        let Some(((x, y), rest)) = points.split_first().map(|(a, b)| (*a, b)) else {
            return;
        };
        if rest.is_empty() {
            self.line((x, y), (x, y), width, color);
            return;
        }
        let rest: Vec<Vector2> = rest.iter().map(|&(x, y)| Vector2 { X: x, Y: y }).collect();
        // SAFETY: the geometry and sink are local COM objects, and the sink is
        // closed before the geometry is drawn.
        let _ = unsafe {
            (|| -> Result<()> {
                let geometry = self.gfx.factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                sink.BeginFigure(Vector2 { X: x, Y: y }, D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLines(&rest);
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                self.target.render.DrawGeometry(
                    &geometry,
                    self.brush(color),
                    width,
                    &self.gfx.round,
                );
                Ok(())
            })()
        };
    }

    /// A path through `points`, closed or open.
    fn path(&self, points: &[(f32, f32)], closed: bool) -> Option<ID2D1PathGeometry> {
        let ((x, y), rest) = points.split_first().map(|(a, b)| (*a, b))?;
        let rest: Vec<Vector2> = rest.iter().map(|&(x, y)| Vector2 { X: x, Y: y }).collect();
        // SAFETY: the geometry and sink are local COM objects, and the sink is
        // closed before the geometry is returned.
        unsafe {
            let geometry = self.gfx.factory.CreatePathGeometry().ok()?;
            let sink = geometry.Open().ok()?;
            let begin = if closed {
                D2D1_FIGURE_BEGIN_FILLED
            } else {
                D2D1_FIGURE_BEGIN_HOLLOW
            };
            sink.BeginFigure(Vector2 { X: x, Y: y }, begin);
            sink.AddLines(&rest);
            sink.EndFigure(if closed {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
            sink.Close().ok()?;
            Some(geometry)
        }
    }

    /// Fills a rounded rectangle whose corners each have their own radius, in
    /// the order top left, top right, bottom right, bottom left.
    pub fn fill_corners(&self, r: Rect, radii: [f32; 4], color: Color) {
        let limit = (r.w / 2.0).min(r.h / 2.0).max(0.0);
        let [tl, tr, br, bl] = radii.map(|radius| radius.clamp(0.0, limit));
        let point = |x: f32, y: f32| Vector2 { X: x, Y: y };
        let arc = |x: f32, y: f32, radius: f32| D2D1_ARC_SEGMENT {
            point: point(x, y),
            size: D2D_SIZE_F {
                width: radius,
                height: radius,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        };
        // SAFETY: the geometry and sink are local COM objects, and the sink is
        // closed before the geometry is drawn.
        let _ = unsafe {
            (|| -> Result<()> {
                let geometry = self.gfx.factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                sink.BeginFigure(point(r.x + tl, r.y), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(point(r.right() - tr, r.y));
                sink.AddArc(&arc(r.right(), r.y + tr, tr));
                sink.AddLine(point(r.right(), r.bottom() - br));
                sink.AddArc(&arc(r.right() - br, r.bottom(), br));
                sink.AddLine(point(r.x + bl, r.bottom()));
                sink.AddArc(&arc(r.x, r.bottom() - bl, bl));
                sink.AddLine(point(r.x, r.y + tl));
                sink.AddArc(&arc(r.x + tl, r.y, tl));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                self.target
                    .render
                    .FillGeometry(&geometry, self.brush(color), None);
                Ok(())
            })()
        };
    }

    /// Strokes a rounded rectangle on the inside of `r`, so a thicker line
    /// never grows the shape.
    pub fn stroke_round(&self, r: Rect, radius: f32, width: f32, color: Color) {
        let half = width / 2.0;
        let corner = (radius.min(r.h / 2.0) - half).max(0.0);
        let shape = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: r.x + half,
                top: r.y + half,
                right: r.right() - half,
                bottom: r.bottom() - half,
            },
            radiusX: corner,
            radiusY: corner,
        };
        // SAFETY: called between BeginDraw and EndDraw.
        unsafe {
            self.target
                .render
                .DrawRoundedRectangle(&shape, self.brush(color), width, None)
        };
    }

    /// Draws a dashed line, `dash` long with `gap` between, with square ends.
    pub fn dashed(
        &self,
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        color: Color,
        (dash, gap): (f32, f32),
    ) {
        // Direct2D measures dashes in stroke widths, so the width is part of
        // what makes a pattern.
        let key = (
            (dash * 100.0) as u32,
            (gap * 100.0) as u32 + ((width * 100.0) as u32) * 100_000,
        );
        let mut styles = self.gfx.dashes.borrow_mut();
        let style = match styles.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let dashes = [dash / width, gap / width];
                let properties = D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_FLAT,
                    endCap: D2D1_CAP_STYLE_FLAT,
                    dashCap: D2D1_CAP_STYLE_FLAT,
                    dashStyle: D2D1_DASH_STYLE_CUSTOM,
                    miterLimit: 1.0,
                    ..Default::default()
                };
                // SAFETY: the properties and dashes are valid for the call.
                let created = unsafe {
                    self.gfx
                        .factory
                        .CreateStrokeStyle(&properties, Some(&dashes))
                };
                let Ok(style) = created else {
                    return;
                };
                entry.insert(style)
            }
        };
        let point = |(x, y): (f32, f32)| Vector2 { X: x, Y: y };
        // SAFETY: called between BeginDraw and EndDraw, and the style lives as
        // long as the `Gfx`.
        unsafe {
            self.target
                .render
                .DrawLine(point(from), point(to), self.brush(color), width, &*style);
        }
    }

    /// Fills the area under a line down to `bottom`, fading from `alpha` at the
    /// line's highest point to nothing `depth` below it.
    pub fn fade(&self, points: &[(f32, f32)], bottom: f32, color: Color, alpha: f32, depth: f32) {
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            return;
        };
        if points.len() < 2 {
            return;
        }
        let top = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let end = bottom.min(top + depth).max(top + 1.0);
        let mut area = Vec::with_capacity(points.len() + 2);
        area.push((first.0, bottom));
        area.extend_from_slice(points);
        area.push((last.0, bottom));
        let Some(geometry) = self.path(&area, true) else {
            return;
        };
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: color.with_alpha(alpha).d2d(),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: color.with_alpha(0.0).d2d(),
            },
        ];
        let line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: Vector2 { X: 0.0, Y: top },
            endPoint: Vector2 { X: 0.0, Y: end },
        };
        // SAFETY: called between BeginDraw and EndDraw; the stops, brush and
        // geometry are local and valid for the calls.
        unsafe {
            let render = &self.target.render;
            let Ok(collection) =
                render.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            else {
                return;
            };
            let Ok(brush) = render.CreateLinearGradientBrush(&line, None, &collection) else {
                return;
            };
            if let Ok(brush) = brush.cast::<ID2D1Brush>() {
                render.FillGeometry(&geometry, &brush, None);
            }
        }
    }

    fn format(&self, size: f32, weight: Weight, align: Align, face: Face) -> Option<Format> {
        let key = FormatKey {
            half_points: (size * 2.0).round() as u16,
            weight,
            align,
            face,
        };
        let mut formats = self.gfx.formats.borrow_mut();
        if let Some(format) = formats.get(&key) {
            return Some(format.clone());
        }
        let font_weight: DWRITE_FONT_WEIGHT = match weight {
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL,
            Weight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        };
        let alignment: DWRITE_TEXT_ALIGNMENT = match align {
            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        };
        let family = match face {
            Face::Text => &self.gfx.families.0,
            Face::Display => &self.gfx.families.1,
        };
        let points = key.half_points as f32 / 2.0;
        // SAFETY: the font name and locale are valid strings, and the layout
        // and out pointers are valid for each call.
        let format = unsafe {
            let format = self
                .gfx
                .write
                .CreateTextFormat(
                    family,
                    None,
                    font_weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    points,
                    w!("en-us"),
                )
                .ok()?;
            format.SetTextAlignment(alignment).ok()?;
            format
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)
                .ok()?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
            // The baseline comes from the font itself, so a fallback font
            // still sits on the line it is given.
            let sample: Vec<u16> = "0".encode_utf16().collect();
            let mut lines = [DWRITE_LINE_METRICS::default()];
            let mut count = 0;
            let baseline = match self.gfx.write.CreateTextLayout(&sample, &format, FAR, FAR) {
                Ok(layout) if layout.GetLineMetrics(Some(&mut lines), &mut count).is_ok() => {
                    lines[0].baseline
                }
                _ => ASCENT * points,
            };
            Format { format, baseline }
        };
        formats.insert(key, format.clone());
        Some(format)
    }

    /// Measures the advance width of `text`.
    pub fn measure(&self, text: &str, size: f32, weight: Weight) -> f32 {
        self.measure_in(Face::Text, text, size, weight)
    }

    /// Measures the advance width of `text` in the given face.
    pub fn measure_in(&self, face: Face, text: &str, size: f32, weight: Weight) -> f32 {
        let Some(format) = self.format(size, weight, Align::Left, face) else {
            return 0.0;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: the text buffer is valid for the call, and the layout is a
        // local COM object.
        unsafe {
            match self
                .gfx
                .write
                .CreateTextLayout(&wide, &format.format, FAR, FAR)
            {
                Ok(layout) if layout.GetMetrics(&mut metrics).is_ok() => {
                    metrics.widthIncludingTrailingWhitespace
                }
                _ => 0.0,
            }
        }
    }

    /// Draws `text` with its baseline at `baseline`, and returns its width.
    pub fn text(
        &self,
        text: &str,
        at: (f32, f32),
        size: f32,
        weight: Weight,
        align: Align,
        color: Color,
    ) -> f32 {
        self.text_in(Face::Text, text, at, size, weight, align, color)
    }

    /// Draws a number in the Display face, semibold, with its baseline at
    /// `baseline`, and returns its width.
    pub fn number(&self, text: &str, at: (f32, f32), size: f32, align: Align, color: Color) -> f32 {
        self.text_in(
            Face::Display,
            text,
            at,
            size,
            Weight::Semibold,
            align,
            color,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_in(
        &self,
        face: Face,
        text: &str,
        (x, baseline): (f32, f32),
        size: f32,
        weight: Weight,
        align: Align,
        color: Color,
    ) -> f32 {
        let Some(format) = self.format(size, weight, align, face) else {
            return 0.0;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        let top = baseline - format.baseline;
        let (left, right) = match align {
            Align::Left => (x, x + FAR),
            Align::Right => (x - FAR, x),
            Align::Center => (x - FAR, x + FAR),
        };
        let area = D2D_RECT_F {
            left,
            top,
            right,
            bottom: top + FAR,
        };
        // SAFETY: called between BeginDraw and EndDraw with valid arguments.
        unsafe {
            self.target.render.DrawText(
                &wide,
                &format.format,
                &area,
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        self.measure_in(face, text, size, weight)
    }

    /// Draws `text` vertically centred on `y`, by its capital letters.
    pub fn text_mid(
        &self,
        text: &str,
        (x, y): (f32, f32),
        size: f32,
        weight: Weight,
        align: Align,
        color: Color,
    ) -> f32 {
        self.text(
            text,
            (x, y + size * CAP_HEIGHT / 2.0),
            size,
            weight,
            align,
            color,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_decode() {
        let c = Color::rgb(0xff0080);
        assert_eq!((c.r, c.g), (1.0, 0.0));
        assert!((c.b - 0.502).abs() < 0.001);
    }
}
