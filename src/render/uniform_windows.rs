//! Frame-scoped uniform windows bound at dynamic offsets (VERA scheduling).
//!
//! A pass that reads a small array of per-draw records binds one window of a
//! frame buffer at its own offset. Queue writes all land before the next
//! submit, so no two passes of a frame share a window; a frame starts again
//! after the previous frame's submits. The first window of every buffer is
//! never written and stays zero ([`UniformWindows::zeros`]).

use std::cell::RefCell;

pub(crate) struct UniformWindows {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    label: &'static str,
    binding: u32,
    /// Bytes one pass binds.
    window: u64,
    alignment: u64,
    slots: RefCell<Slots>,
}

struct Slots {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    used: u64,
}

impl UniformWindows {
    /// Windows of `window` bytes at `binding` of a layout of their own, with
    /// room for `windows` passes before the buffer first grows.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        binding: u32,
        visibility: wgpu::ShaderStages,
        window: u64,
        windows: u64,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding,
                visibility,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(window),
                },
                count: None,
            }],
        });
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let size = (windows.max(1) + 1) * window.next_multiple_of(alignment);
        let slots = Slots::new(device, &layout, label, binding, window, alignment, size);
        Self {
            device: device.clone(),
            queue: queue.clone(),
            layout,
            label,
            binding,
            window,
            alignment,
            slots: RefCell::new(slots),
        }
    }

    pub(crate) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Start a frame: its passes reuse the buffer after the zero window.
    pub(crate) fn begin_frame(&mut self) {
        self.slots.get_mut().used = self.window.next_multiple_of(self.alignment);
    }

    /// A window of zeros, for a pass whose records would all be zero.
    pub(crate) fn zeros(&self) -> (wgpu::BindGroup, u32) {
        (self.slots.borrow().bind_group.clone(), 0)
    }

    /// Write one pass's records to a fresh window of this frame: the binding
    /// and its dynamic offset. Grows the buffer when it is full; earlier
    /// passes keep the buffer they bound.
    pub(crate) fn write(&self, records: &[u8]) -> (wgpu::BindGroup, u32) {
        assert!(
            records.len() as u64 <= self.window,
            "records fit one window"
        );
        let mut slots = self.slots.borrow_mut();
        if slots.used + self.window > slots.buffer.size() {
            *slots = Slots::new(
                &self.device,
                &self.layout,
                self.label,
                self.binding,
                self.window,
                self.alignment,
                slots.buffer.size() * 2,
            );
        }
        let offset = slots.used;
        self.queue.write_buffer(&slots.buffer, offset, records);
        slots.used = (offset + records.len() as u64).next_multiple_of(self.alignment);
        (slots.bind_group.clone(), offset as u32)
    }
}

impl Slots {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        label: &'static str,
        binding: u32,
        window: u64,
        alignment: u64,
        size: u64,
    ) -> Self {
        // wgpu zero-initialises the buffer, so its unwritten first window
        // reads as zeros.
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(window),
                }),
            }],
        });
        Self {
            buffer,
            bind_group,
            used: window.next_multiple_of(alignment),
        }
    }
}
