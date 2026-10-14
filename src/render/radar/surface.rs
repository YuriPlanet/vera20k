//! Retained ordinary radar pixels, in the native 168×110 housing rectangle.
//!
//! SidebarSurface is allocated/cleared at533FD0, then653100 paints radar
//! frames in place. Leaf4912B0 skips source index0;6A70E0 only copies this
//! surface outward. Online656EC0 writes minimap content into the same surface.

use crate::render::batch::SpriteInstance;

pub(in crate::render) struct RadarSurface {
    pub rgba: Vec<u8>,
    width: u32,
    height: u32,
}
impl RadarSurface {
    pub fn new(width: u32, height: u32) -> Self {
        let mut rgba = vec![0; (width * height * 4) as usize];
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        Self {
            rgba,
            width,
            height,
        }
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.width, self.height]
    }
    pub fn paint_movie(&mut self, source: &[u8], size: [u32; 2]) {
        assert_eq!(source.len(), (size[0] * size[1] * 4) as usize);
        let x_origin = self.width.saturating_sub(size[0]) / 2;
        for y in 0..size[1].min(self.height) {
            let width = size[0].min(self.width - x_origin);
            let from = (y * size[0] * 4) as usize;
            let to = ((y * self.width + x_origin) * 4) as usize;
            self.rgba[to..to + width as usize * 4]
                .copy_from_slice(&source[from..from + width as usize * 4]);
        }
    }
    pub fn paint_frame(&mut self, frame: &[u8]) {
        assert_eq!(frame.len(), self.rgba.len());
        for (dest, source) in self
            .rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(frame.as_chunks::<4>().0)
        {
            if source[3] != 0 {
                dest.copy_from_slice(source);
            }
        }
    }
    /// Retain already-prepared, opaque UI draws. Source/rect authority stays
    /// in MinimapRenderer; this copies its actual quad, crop and nearest pixels.
    /// World camera compensation is removed once at this surface boundary.
    pub fn paint_quad(
        &mut self,
        quad: &SpriteInstance,
        origin: [f32; 2],
        source: &[u8],
        source_size: [u32; 2],
    ) {
        assert_eq!(source.len(), (source_size[0] * source_size[1] * 4) as usize);
        // Match the production batch vertex shader's native-zoom snap.
        let p = [
            (quad.position[0] - origin[0] + 0.5).floor(),
            (quad.position[1] - origin[1] + 0.5).floor(),
        ];
        let size = [
            (quad.position[0] + quad.size[0] - origin[0] + 0.5).floor() - p[0],
            (quad.position[1] + quad.size[1] - origin[1] + 0.5).floor() - p[1],
        ];
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return;
        }
        let start = [
            (p[0] - 0.5).ceil().max(0.) as u32,
            (p[1] - 0.5).ceil().max(0.) as u32,
        ];
        let end = [
            (p[0] + size[0] - 0.5).ceil().clamp(0., self.width as f32) as u32,
            (p[1] + size[1] - 0.5).ceil().clamp(0., self.height as f32) as u32,
        ];
        for y in start[1]..end[1] {
            for x in start[0]..end[0] {
                let uv = [
                    quad.uv_origin[0] + (x as f32 + 0.5 - p[0]) / size[0] * quad.uv_size[0],
                    quad.uv_origin[1] + (y as f32 + 0.5 - p[1]) / size[1] * quad.uv_size[1],
                ];
                let sx = (uv[0] * source_size[0] as f32)
                    .floor()
                    .clamp(0., (source_size[0] - 1) as f32) as u32;
                let sy = (uv[1] * source_size[1] as f32)
                    .floor()
                    .clamp(0., (source_size[1] - 1) as f32) as u32;
                let from = ((sy * source_size[0] + sx) * 4) as usize;
                let to = ((y * self.width + x) * 4) as usize;
                if source[from + 3] != 0 {
                    self.rgba[to..to + 4].copy_from_slice(&source[from..from + 4]);
                }
            }
        }
    }
    pub fn paint_solid(&mut self, quad: &SpriteInstance, origin: [f32; 2]) {
        let pixel = [
            (quad.tint[0] * 255.).round() as u8,
            (quad.tint[1] * 255.).round() as u8,
            (quad.tint[2] * 255.).round() as u8,
            255,
        ];
        self.paint_quad(quad, origin, &pixel, [1, 1]);
    }
}
