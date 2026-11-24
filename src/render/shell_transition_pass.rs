//! Shell render-target bundle.
//!
//! A borrowed (color, depth) view pair handed to the shell renderers so they
//! can draw to the swapchain or an offscreen target uniformly.

pub(crate) struct ShellRenderTarget<'a> {
    pub(crate) color: &'a wgpu::TextureView,
    pub(crate) depth: &'a wgpu::TextureView,
}
