//! The finished terrain layer, kept between frames.
//!
//! VERA scheduling only; no native counterpart. Terrain tiles, smudges and
//! cell bodies are the frame's widest draws, and their pixels are a pure
//! function of [`TerrainInputs`]. While those hold still, the layer's colour
//! and depth come back by copy instead of being drawn again. The copy out
//! waits for the second equal frame.
//!
//! A moved camera reuses a kept copy of the ground, the tiles and smudges
//! under the cell bodies. The move is whole pixels, so the ground moves with
//! it unchanged: the frame draws only the strips it exposed, and the depth
//! moves through [`DepthShift`]. Cell bodies stay out of that copy and draw
//! every frame, since their Z seeds round to thirds of the camera's row
//! (`native_row_z`) and so do not move with it. What the kept ground was
//! drawn from is compared with this frame's, over the pixels it still shows;
//! anything else in the frame that changed draws the ground in full.

use crate::app::presentation::instances::CellBodyDraw;
use crate::render::batch::{CameraUniform, SpriteInstance};
use crate::render::depth_shift::DepthShift;
use crate::render::shroud_buffer::{KeptPlane, ShroudBuffer};

/// Every instance stream the terrain layer draws.
pub(super) const TERRAIN_STREAMS: [&str; 3] = ["terrain", "smudge", "cell_body"];

const COPIES: wgpu::TextureUsages =
    wgpu::TextureUsages::COPY_SRC.union(wgpu::TextureUsages::COPY_DST);

/// The fewest frames between copies of the ground that could not be reused,
/// so one that changes every frame costs a fraction of a copy per frame.
const GROUND_KEEP_INTERVAL: u32 = 4;

/// What the terrain layer's pixels are drawn from.
#[derive(PartialEq)]
pub(super) struct TerrainInputs {
    /// The generation of each of [`TERRAIN_STREAMS`].
    pub(super) streams: [u64; 3],
    pub(super) cell_bodies: Vec<CellBodyDraw>,
    pub(super) camera: CameraUniform,
    /// Group 0 of the world pipelines, and the generation of the A plane it
    /// samples.
    pub(super) camera_group: wgpu::BindGroup,
    pub(super) a_plane: u64,
    pub(super) tactical: [u32; 4],
    pub(super) atlases: [Option<wgpu::BindGroup>; 4],
    /// The colour and depth targets it is drawn into.
    pub(super) targets: [wgpu::Texture; 2],
}

/// This frame's instances and A plane under the ground.
#[derive(Clone, Copy)]
pub(super) struct GroundFrame<'a> {
    pub(super) terrain: &'a [SpriteInstance],
    pub(super) smudge: &'a [SpriteInstance],
    pub(super) a_plane: Option<&'a ShroudBuffer>,
}

/// How a frame gets its terrain layer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TerrainStep {
    /// Draw it, keeping a copy of the ground and of the whole layer.
    Draw { keep_ground: bool, keep_whole: bool },
    /// [`TerrainPicture::shift`] the kept ground in, then draw its
    /// [`GroundShift::strips`] and the cell bodies.
    Shift(GroundShift),
    /// [`TerrainPicture::restore`] the kept copy.
    Restore,
}

impl TerrainStep {
    /// The target rects, some possibly empty, this step draws ground into.
    pub(super) fn ground_areas(self, tactical: [u32; 4]) -> [[u32; 4]; 2] {
        match self {
            Self::Draw { .. } => [tactical, [0; 4]],
            Self::Shift(shift) => shift.strips,
            Self::Restore => [[0; 4]; 2],
        }
    }
}

/// How the kept ground lands in the targets under a moved camera. Rects are
/// x, y, width, height in target pixels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct GroundShift {
    /// The target pixels the kept ground still shows.
    overlap: [u32; 4],
    /// The target pixel `p` shows the kept pixel `p + offset`: the camera's
    /// move in whole pixels.
    offset: [i32; 2],
    /// The rest of the tactical rect, which the kept ground lacks; either
    /// may be empty.
    strips: [[u32; 4]; 2],
}

impl GroundShift {
    /// The shift for a camera moved by `[dx, dy]` over the tactical rect, if
    /// the kept ground still shows at least three quarters of it.
    fn between([x, y, width, height]: [u32; 4], [dx, dy]: [i32; 2]) -> Option<Self> {
        let (width, height) = (width as i32, height as i32);
        let (left, right) = ((-dx).max(0), width - dx.max(0));
        let (top, bottom) = ((-dy).max(0), height - dy.max(0));
        if right <= left || bottom <= top {
            return None;
        }
        let tactical = i64::from(width) * i64::from(height);
        let shown = i64::from(right - left) * i64::from(bottom - top);
        if (tactical - shown) * 4 > tactical {
            return None;
        }
        let rect = |l: i32, t: i32, r: i32, b: i32| {
            [x + l as u32, y + t as u32, (r - l) as u32, (b - t) as u32]
        };
        Some(Self {
            overlap: rect(left, top, right, bottom),
            offset: [dx, dy],
            strips: [
                if dx > 0 {
                    rect(right, 0, width, height)
                } else {
                    rect(0, 0, left, height)
                },
                if dy > 0 {
                    rect(left, bottom, right, height)
                } else {
                    rect(left, 0, right, top)
                },
            ],
        })
    }

    /// The world pixels (left, top, right, bottom) `overlap` shows under
    /// `camera`, whose owner rounds it to whole pixels
    /// (`BatchRenderer::update_camera_on_queue`).
    fn world_area(&self, camera: [f32; 2]) -> [i32; 4] {
        let [x, y, width, height] = self.overlap.map(|side| side as i32);
        let [camera_x, camera_y] = camera.map(|side| side as i32);
        [
            x + camera_x,
            y + camera_y,
            x + width + camera_x,
            y + height + camera_y,
        ]
    }
}

/// Whether two instance lists draw the same on the world pixels `area`
/// (left, top, right, bottom): the same quads over it, in the same order.
/// The quads' edges snap as the shaders snap them at zoom 1, so this is
/// exact.
fn same_quads_over(area: [i32; 4], kept: &[SpriteInstance], now: &[SpriteInstance]) -> bool {
    let over = |quad: &&SpriteInstance| {
        let snapped = |axis: usize| {
            let (start, end) = (quad.position[axis], quad.position[axis] + quad.size[axis]);
            let (start, end) = ((start + 0.5).floor(), (end + 0.5).floor());
            (start.min(end), start.max(end))
        };
        let (left, right) = snapped(0);
        let (top, bottom) = snapped(1);
        left < area[2] as f32
            && right > area[0] as f32
            && top < area[3] as f32
            && bottom > area[1] as f32
    };
    kept.iter()
        .filter(over)
        .map(bytemuck::bytes_of)
        .eq(now.iter().filter(over).map(bytemuck::bytes_of))
}

fn fits(kept: &wgpu::Texture, target: &wgpu::Texture) -> bool {
    kept.size() == target.size() && kept.format() == target.format()
}

fn kept_texture(
    device: &wgpu::Device,
    label: &str,
    target: &wgpu::Texture,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: target.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: target.format(),
        usage,
        view_formats: &[],
    })
}

/// A copy of the ground, and what it was drawn from.
struct KeptGround {
    /// Colour and depth.
    textures: [wgpu::Texture; 2],
    /// The [`DepthShift`] binding of the depth.
    depth_binding: wgpu::BindGroup,
    camera: CameraUniform,
    camera_group: wgpu::BindGroup,
    tactical: [u32; 4],
    atlases: [Option<wgpu::BindGroup>; 4],
    terrain: Vec<SpriteInstance>,
    smudge: Vec<SpriteInstance>,
    a_plane: Option<KeptPlane>,
}

impl KeptGround {
    fn record(&mut self, inputs: &TerrainInputs, ground: GroundFrame<'_>) {
        self.camera = inputs.camera;
        self.camera_group.clone_from(&inputs.camera_group);
        self.tactical = inputs.tactical;
        self.atlases.clone_from(&inputs.atlases);
        self.terrain.clear();
        self.terrain.extend_from_slice(ground.terrain);
        self.smudge.clear();
        self.smudge.extend_from_slice(ground.smudge);
        self.a_plane = ground.a_plane.map(|plane| {
            let mut kept = self.a_plane.take().unwrap_or_default();
            plane.keep(&mut kept);
            kept
        });
    }

    /// How a frame drawing `inputs` and `ground` can reuse this copy, if it
    /// can: the camera only moved, and everything drawn on the pixels the
    /// copy still shows is what it was.
    fn shift_for(&self, inputs: &TerrainInputs, ground: GroundFrame<'_>) -> Option<GroundShift> {
        let unmoved = |camera: &CameraUniform| CameraUniform {
            camera_pos: [0.0; 2],
            ..*camera
        };
        if inputs.camera.zoom != 1.0
            || unmoved(&inputs.camera) != unmoved(&self.camera)
            || inputs.camera_group != self.camera_group
            || inputs.tactical != self.tactical
            || inputs.atlases != self.atlases
            || !fits(&self.textures[0], &inputs.targets[0])
            || !fits(&self.textures[1], &inputs.targets[1])
        {
            return None;
        }
        let camera = inputs.camera.camera_pos;
        let shift = GroundShift::between(
            inputs.tactical,
            [
                (camera[0] - self.camera.camera_pos[0]) as i32,
                (camera[1] - self.camera.camera_pos[1]) as i32,
            ],
        )?;
        let area = shift.world_area(camera);
        // A plane rounded apart from the camera addresses one pixel over at
        // the view's edge, so neither end may be.
        let tracks = |origin: [i32; 2], camera: [f32; 2]| origin.map(|v| v as f32) == camera;
        let plane_same = match (&self.a_plane, ground.a_plane) {
            (Some(kept), Some(plane)) => {
                tracks(kept.origin(), self.camera.camera_pos)
                    && tracks(plane.world_origin(), camera)
                    && plane.shows_kept(kept, area)
            }
            (None, None) => true,
            _ => false,
        };
        (plane_same
            && same_quads_over(area, &self.terrain, ground.terrain)
            && same_quads_over(area, &self.smudge, ground.smudge))
        .then_some(shift)
    }
}

#[derive(Default)]
pub(crate) struct TerrainPicture {
    /// The inputs of the last frame's layer.
    drawn: Option<TerrainInputs>,
    /// Textures for the kept colour and depth of the whole layer.
    kept: Option<[wgpu::Texture; 2]>,
    /// Whether `kept` holds the layer `drawn` describes.
    valid: bool,
    ground: Option<KeptGround>,
    depth_shift: Option<DepthShift>,
    /// Frames since `ground` was last copied.
    frames_since_keep: u32,
}

impl TerrainPicture {
    /// How this frame gets its layer. The WebGL fallback and a target that
    /// cannot be copied always draw it.
    pub(super) fn step(
        &mut self,
        device: &wgpu::Device,
        inputs: TerrainInputs,
        ground: GroundFrame<'_>,
    ) -> TerrainStep {
        let copyable = !crate::render::gpu::webgl_fallback(device)
            && inputs
                .targets
                .iter()
                .all(|target| target.usage().contains(COPIES));
        if !copyable {
            self.valid = false;
            self.drawn = None;
            return TerrainStep::Draw {
                keep_ground: false,
                keep_whole: false,
            };
        }
        if self.drawn.as_ref() == Some(&inputs) {
            return if self.valid {
                TerrainStep::Restore
            } else {
                TerrainStep::Draw {
                    keep_ground: true,
                    keep_whole: true,
                }
            };
        }
        self.valid = false;
        self.frames_since_keep = self.frames_since_keep.saturating_add(1);
        let shift = self
            .ground
            .as_ref()
            .and_then(|kept| kept.shift_for(&inputs, ground));
        self.drawn = Some(inputs);
        match shift {
            Some(shift) => TerrainStep::Shift(shift),
            None => TerrainStep::Draw {
                keep_ground: self.frames_since_keep >= GROUND_KEEP_INTERVAL,
                keep_whole: false,
            },
        }
    }

    /// Copy the ground just drawn out of its targets. No pass may be open.
    pub(super) fn keep_ground(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        ground: GroundFrame<'_>,
    ) {
        let inputs = self
            .drawn
            .as_ref()
            .expect("the ground to keep was just stepped");
        let targets = &inputs.targets;
        let shifter = self
            .depth_shift
            .get_or_insert_with(|| DepthShift::new(device, targets[1].format()));
        let mut kept = self
            .ground
            .take()
            .filter(|kept| {
                fits(&kept.textures[0], &targets[0]) && fits(&kept.textures[1], &targets[1])
            })
            .unwrap_or_else(|| {
                let textures = [
                    kept_texture(device, "Kept ground colour", &targets[0], COPIES),
                    kept_texture(
                        device,
                        "Kept ground depth",
                        &targets[1],
                        COPIES | wgpu::TextureUsages::TEXTURE_BINDING,
                    ),
                ];
                let depth_binding = shifter.bind(device, &textures[1]);
                KeptGround {
                    textures,
                    depth_binding,
                    camera: inputs.camera,
                    camera_group: inputs.camera_group.clone(),
                    tactical: inputs.tactical,
                    atlases: inputs.atlases.clone(),
                    terrain: Vec::new(),
                    smudge: Vec::new(),
                    a_plane: None,
                }
            });
        for (target, kept) in targets.iter().zip(&kept.textures) {
            encoder.copy_texture_to_texture(
                target.as_image_copy(),
                kept.as_image_copy(),
                target.size(),
            );
        }
        kept.record(inputs, ground);
        self.ground = Some(kept);
        self.frames_since_keep = 0;
    }

    /// Bring the kept ground into the cleared targets under the moved
    /// camera. No pass may be open.
    pub(super) fn shift(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        shift: GroundShift,
    ) {
        let drawn = self.drawn.as_ref().expect("a shift has inputs");
        let kept = self.ground.as_ref().expect("a shift has a kept ground");
        let [x, y, width, height] = shift.overlap;
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: (x as i32 + shift.offset[0]) as u32,
                    y: (y as i32 + shift.offset[1]) as u32,
                    z: 0,
                },
                ..kept.textures[0].as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d { x, y, z: 0 },
                ..drawn.targets[0].as_image_copy()
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.depth_shift
            .as_ref()
            .expect("a kept ground made its depth shift")
            .encode(
                queue,
                encoder,
                depth,
                &kept.depth_binding,
                shift.overlap,
                shift.offset,
                shift.offset[1],
            );
    }

    /// Copy the layer just drawn out of its targets. No pass may be open.
    pub(super) fn keep(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let targets = &self
            .drawn
            .as_ref()
            .expect("the layer to keep was just stepped")
            .targets;
        if !self
            .kept
            .as_ref()
            .is_some_and(|kept| fits(&kept[0], &targets[0]) && fits(&kept[1], &targets[1]))
        {
            self.kept = Some(
                targets
                    .each_ref()
                    .map(|target| kept_texture(device, "Kept terrain layer", target, COPIES)),
            );
        }
        let kept = self.kept.as_ref().expect("kept textures were just ensured");
        for (target, kept) in targets.iter().zip(kept) {
            encoder.copy_texture_to_texture(
                target.as_image_copy(),
                kept.as_image_copy(),
                target.size(),
            );
        }
        self.valid = true;
    }

    /// Copy the kept layer into its targets. No pass may be open.
    pub(super) fn restore(&self, encoder: &mut wgpu::CommandEncoder) {
        let targets = &self
            .drawn
            .as_ref()
            .expect("a kept layer has inputs")
            .targets;
        let kept = self.kept.as_ref().expect("a kept layer has textures");
        for (target, kept) in targets.iter().zip(kept) {
            encoder.copy_texture_to_texture(
                kept.as_image_copy(),
                target.as_image_copy(),
                target.size(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::gpu::block_on;
    use bytemuck::Zeroable;

    const NO_GROUND: GroundFrame<'static> = GroundFrame {
        terrain: &[],
        smudge: &[],
        a_plane: None,
    };
    const DRAW: TerrainStep = TerrainStep::Draw {
        keep_ground: false,
        keep_whole: false,
    };
    const DRAW_AND_KEEP: TerrainStep = TerrainStep::Draw {
        keep_ground: true,
        keep_whole: true,
    };

    #[test]
    fn a_camera_move_shows_the_kept_ground_and_exposes_strips() {
        let tactical = [10, 20, 100, 100];
        let shift = GroundShift::between(tactical, [10, -5]).unwrap();
        assert_eq!(shift.overlap, [10, 25, 90, 95]);
        assert_eq!(shift.offset, [10, -5]);
        assert_eq!(shift.strips, [[100, 20, 10, 100], [10, 20, 90, 5]]);
        assert_eq!(shift.world_area([1000.0, 500.0]), [1010, 525, 1100, 620]);

        let shift = GroundShift::between(tactical, [-3, 7]).unwrap();
        assert_eq!(shift.overlap, [13, 20, 97, 93]);
        assert_eq!(shift.strips, [[10, 20, 3, 100], [13, 113, 97, 7]]);

        let unmoved = GroundShift::between(tactical, [0, 0]).unwrap();
        assert_eq!(unmoved.overlap, tactical);
        assert_eq!(unmoved.strips.map(|strip| strip[2] * strip[3]), [0, 0]);

        // Exactly a quarter exposed still reuses it; more draws it in full.
        assert!(GroundShift::between(tactical, [25, 0]).is_some());
        assert!(GroundShift::between(tactical, [26, 0]).is_none());
        assert!(GroundShift::between(tactical, [0, -100]).is_none());
    }

    #[test]
    fn ground_quads_compare_over_the_kept_area_alone() {
        let quad = |x, y, tint| SpriteInstance {
            position: [x, y],
            size: [60.0, 30.0],
            tint: [tint; 3],
            ..Default::default()
        };
        let area = [100, 100, 200, 200];
        let (on, off) = (quad(90.0, 90.0, 1.0), quad(300.0, 90.0, 1.0));
        let same =
            |kept: &[SpriteInstance], now: &[SpriteInstance]| same_quads_over(area, kept, now);
        assert!(same(&[off, on], &[on]));
        assert!(!same(&[on], &[quad(90.0, 90.0, 0.5)]));
        assert!(!same(&[on], &[]));
        // The same two quads over the area in the other order draw otherwise.
        let other = quad(120.0, 120.0, 1.0);
        assert!(!same(&[on, other], &[other, on]));
        // Edges snap to the nearest pixel: one ending at 100.4 ends at 100,
        // short of the area, and one ending at 100.5 covers its first column.
        let ends_at = |x: f32| quad(x - 60.0, 120.0, 1.0);
        assert!(same(&[ends_at(100.4)], &[]));
        assert!(!same(&[ends_at(100.5)], &[]));
    }

    #[test]
    #[ignore = "requires a wgpu adapter; executes the production ground shift"]
    fn a_moved_camera_brings_back_the_ground_with_its_depth_moved() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("explicit terrain picture GPU test requires adapter");
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        const SIDE: u32 = 16;
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | COPIES,
            view_formats: &[],
        });
        let color_view = color.create_view(&Default::default());
        let (depth, depth_view) =
            crate::render::scaled_copy::create_depth_target(&device, "Test", (SIDE, SIDE));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[],
        });
        let inputs = |camera_pos| TerrainInputs {
            streams: [0; 3],
            cell_bodies: Vec::new(),
            camera: CameraUniform {
                camera_pos,
                zoom: 1.0,
                ..CameraUniform::zeroed()
            },
            camera_group: group.clone(),
            a_plane: 0,
            tactical: [0, 0, SIDE, SIDE],
            atlases: [None, None, None, None],
            targets: [color.clone(), depth.clone()],
        };
        let clear_depth = |encoder: &mut wgpu::CommandEncoder, z: f32| {
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(z),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
        };

        // Each colour pixel holds its own coordinates.
        let pixels: Vec<u8> = (0..SIDE as u8)
            .flat_map(|y| (0..SIDE as u8).flat_map(move |x| [x, y, 0, 255]))
            .collect();
        queue.write_texture(
            color.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: None,
            },
            color.size(),
        );
        // Keep a ground of one Z, shift its left half over an empty target
        // with the production pass, and keep that: Z on the left, empty on
        // the right.
        let stored = |z: i32| z as f32 / 65535.0;
        let mut picture = TerrainPicture::default();
        assert!(picture.step(&device, inputs([100.0, 200.0]), NO_GROUND) == DRAW);
        assert!(picture.step(&device, inputs([100.0, 200.0]), NO_GROUND) == DRAW_AND_KEEP);
        let mut encoder = device.create_command_encoder(&Default::default());
        clear_depth(&mut encoder, stored(32773));
        picture.keep_ground(&device, &mut encoder, NO_GROUND);
        clear_depth(&mut encoder, 1.0);
        picture.shift(
            &queue,
            &mut encoder,
            &depth_view,
            GroundShift {
                overlap: [0, 0, SIDE / 2, SIDE],
                offset: [0, 0],
                strips: [[0; 4]; 2],
            },
        );
        picture.keep_ground(&device, &mut encoder, NO_GROUND);
        queue.submit([encoder.finish()]);

        let step = picture.step(&device, inputs([102.0, 201.0]), NO_GROUND);
        let TerrainStep::Shift(shift) = step else {
            panic!("a small move of an unchanged ground reuses it");
        };
        assert_eq!(shift.overlap, [0, 0, SIDE - 2, SIDE - 1]);
        assert_eq!(
            shift.strips,
            [[SIDE - 2, 0, 2, SIDE], [0, SIDE - 1, SIDE - 2, 1]]
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        picture.shift(&queue, &mut encoder, &depth_view, shift);

        let readback = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 256 * u64::from(SIDE),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        };
        let (color_read, depth_read) = (readback("Colour"), readback("Depth"));
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256),
            rows_per_image: None,
        };
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &color_read,
                layout,
            },
            color.size(),
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::DepthOnly,
                ..depth.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &depth_read,
                layout,
            },
            color.size(),
        );
        queue.submit([encoder.finish()]);
        for buffer in [&color_read, &depth_read] {
            buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let colors = color_read.slice(..).get_mapped_range().unwrap();
        let depths = depth_read.slice(..).get_mapped_range().unwrap();
        let at = |x: usize, y: usize| {
            let offset = y * 256 + x * 4;
            let word: [u8; 4] = depths[offset..offset + 4].try_into().unwrap();
            (
                colors[offset..offset + 4].to_vec(),
                (f32::from_ne_bytes(word) * 65535.0).round() as i32,
            )
        };
        // Target (5, 0) shows kept (7, 1), the left half's last column: one
        // row more of camera is one more Z.
        assert_eq!(at(5, 0), (vec![7, 1, 0, 255], 32774));
        // Target (6, 0) shows kept (8, 1): nothing wrote its depth.
        assert_eq!(at(6, 0), (vec![8, 1, 0, 255], 65535));
        // The strips stay as the clear left them.
        assert_eq!(at(SIDE as usize - 1, 0), (vec![0, 0, 0, 255], 65535));
        assert_eq!(at(0, SIDE as usize - 1), (vec![0, 0, 0, 255], 65535));
    }

    #[test]
    #[ignore = "requires a wgpu adapter; executes the production terrain copies"]
    fn the_second_equal_frame_keeps_the_layer_and_the_third_restores_it() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("explicit terrain picture GPU test requires adapter");
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | COPIES,
            view_formats: &[],
        });
        let (depth, depth_view) =
            crate::render::scaled_copy::create_depth_target(&device, "Test", (4, 4));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[],
        });
        let inputs = |stream| TerrainInputs {
            streams: [stream, 0, 0],
            cell_bodies: Vec::new(),
            camera: CameraUniform::zeroed(),
            camera_group: group.clone(),
            a_plane: 0,
            tactical: [0, 0, 4, 4],
            atlases: [None, None, None, None],
            targets: [color.clone(), depth.clone()],
        };
        let color_view = color.create_view(&Default::default());
        let clear = |encoder: &mut wgpu::CommandEncoder, g: f64, z: f32| {
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(z),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
        };

        let mut picture = TerrainPicture::default();
        assert!(picture.step(&device, inputs(1), NO_GROUND) == DRAW);
        assert!(picture.step(&device, inputs(1), NO_GROUND) == DRAW_AND_KEEP);
        let mut encoder = device.create_command_encoder(&Default::default());
        clear(&mut encoder, 1.0, 0.25);
        picture.keep(&device, &mut encoder);
        clear(&mut encoder, 0.0, 0.75);
        assert!(picture.step(&device, inputs(1), NO_GROUND) == TerrainStep::Restore);
        picture.restore(&mut encoder);

        let bytes = 256 * 4;
        let readback = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        };
        let (color_read, depth_read) = (readback("Colour"), readback("Depth"));
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256),
            rows_per_image: None,
        };
        let size = color.size();
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &color_read,
                layout,
            },
            size,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::DepthOnly,
                ..depth.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &depth_read,
                layout,
            },
            size,
        );
        queue.submit([encoder.finish()]);
        for buffer in [&color_read, &depth_read] {
            buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(
            color_read.slice(..4).get_mapped_range().unwrap()[..],
            [0, 255, 0, 255]
        );
        assert_eq!(
            depth_read.slice(..4).get_mapped_range().unwrap()[..],
            0.25_f32.to_ne_bytes()
        );

        assert!(picture.step(&device, inputs(2), NO_GROUND) == DRAW);
    }
}
