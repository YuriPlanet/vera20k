//! The GPU voxel raster read back against the CPU raster it stands for.

use std::collections::BTreeSet;

use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::terrain_draw_gpu_tests::Gpu;
use crate::render::vxl_raster::{self, BodyTilt, VxlRenderParams, VxlSprite};
use crate::rules::voxel_asset_catalog::VoxelPart;

const PAGE: u32 = 1024;
/// Each image's cell on a test page: the largest crop and its padding.
const CELL: u32 = 257;

/// Draw `images` nine to a page and read each back at its size.
fn draw(
    gpu: &Gpu,
    batch: &BatchRenderer,
    raster: &mut VoxelGpu,
    images: &[(GpuImage, [u32; 2])],
    vpl: Option<&VplFile>,
) -> Vec<Vec<u8>> {
    let corner = |index: usize| [(index % 3) as u32 * CELL, (index / 3) as u32 * CELL];
    let mut read = Vec::new();
    for chunk in images.chunks(9) {
        let page = batch.create_unit_page_texture(&gpu.device, PAGE, PAGE);
        let pass = PagePass {
            target: &page.view,
            images: chunk
                .iter()
                .enumerate()
                .map(|(index, (image, _))| (corner(index), image))
                .collect(),
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        raster.encode(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            [PAGE; 2],
            &[pass],
            vpl,
        );
        gpu.queue.submit([encoder.finish()]);
        let texels = gpu.read_uint_texels(&page.view, [0, 0], [3 * CELL, 3 * CELL]);
        for (index, (_, [width, height])) in chunk.iter().enumerate() {
            let [x, y] = corner(index);
            read.push(
                (y..y + height)
                    .flat_map(|row| {
                        let at = (row * 3 * CELL + x) as usize;
                        texels[at..at + *width as usize].iter().copied()
                    })
                    .collect(),
            );
        }
    }
    read
}

fn crop(sprite: &VxlSprite) -> [u32; 2] {
    [sprite.width, sprite.height]
}

/// Ordinary draws of retail bodies across facings and tilts take every
/// raster orientation (mirrored X and Y columns, backward walks); the GPU
/// image of each is the CPU raster's, stores of zero included.
#[test]
#[ignore = "requires a wgpu adapter and retail assets"]
fn retail_paints_match_the_cpu_raster_in_every_orientation() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let vpl = VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap()).unwrap();
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut raster = VoxelGpu::default();
    let mut images = Vec::new();
    let mut expected = Vec::new();
    let mut orientations = BTreeSet::new();
    for name in ["MTNK", "ZEP", "ORCA", "DRED"] {
        let part = VoxelPart::load(&assets, name, false).unwrap();
        for facing in (0..=255u8).step_by(32) {
            for tilt in [
                BodyTilt::Fly([0.0, 0.0]),
                BodyTilt::Fly([0.4, -0.3]),
                BodyTilt::Fly([2.6, 0.2]),
                BodyTilt::Fly([3.1, 1.0]),
                BodyTilt::Rocket(1.2),
            ] {
                let params = VxlRenderParams {
                    facing,
                    body_tilt: Some(tilt),
                    ..VxlRenderParams::default()
                };
                let image = vxl_raster::voxel_image(part.vxl(), part.hva(), &params, Some(&vpl));
                let admitted = raster
                    .admit(&image, Some(&vpl))
                    .unwrap_or_else(|| panic!("{name} {facing} {tilt:?} draws on the GPU"));
                let GpuImage::Paint { sections, .. } = &admitted else {
                    unreachable!("a paint");
                };
                orientations.extend(sections.iter().map(|(_, params)| {
                    (
                        params.column_steps[1] < 0,
                        params.column_steps[2] < 0,
                        params.reverse,
                    )
                }));
                let sprite = vxl_raster::render_vxl(part.vxl(), part.hva(), &params, Some(&vpl));
                images.push((admitted, crop(&sprite)));
                expected.push(((name, facing, tilt), sprite.palette_indices));
            }
        }
    }
    assert_eq!(orientations.len(), 8, "{orientations:?}");
    let read = draw(&gpu, &batch, &mut raster, &images, Some(&vpl));
    for ((case, pixels), gpu_pixels) in expected.iter().zip(read) {
        assert_eq!(&gpu_pixels, pixels, "{case:?}");
    }
}

/// A debris piece's ground stencil (756860) marks two adjacent bytes per
/// column on the GPU as on the CPU.
#[test]
#[ignore = "requires a wgpu adapter and retail assets"]
fn retail_stencils_match_the_cpu_raster() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut raster = VoxelGpu::default();
    let mut images = Vec::new();
    let mut expected = Vec::new();
    for name in ["TIRE", "PIECE"] {
        let part = VoxelPart::load(&assets, name, false).unwrap();
        for velocity in [[1, 0, 0], [3, -2, 1], [-1, 4, -3], [0, -5, 2], [2, 2, -4]] {
            let [x, y, z] = velocity;
            let matrix = vxl_raster::projectile_voxel_matrix(
                crate::sim::projectile::ProjectileVelocity::new(x, y, z),
            )
            .unwrap();
            let image = vxl_raster::shadow::matrix_image(part.vxl(), matrix, 0, false).unwrap();
            let admitted = raster
                .admit(&image, None)
                .unwrap_or_else(|| panic!("{name} {velocity:?} marks on the GPU"));
            let sprite = vxl_raster::shadow::render_matrix(part.vxl(), matrix, 0, false).unwrap();
            images.push((admitted, crop(&sprite)));
            expected.push(((name, velocity), sprite.palette_indices));
        }
    }
    let read = draw(&gpu, &batch, &mut raster, &images, None);
    for ((case, pixels), gpu_pixels) in expected.iter().zip(read) {
        assert_eq!(&gpu_pixels, pixels, "{case:?}");
    }

    // VoxelAnim749CCF/749DCE: one selected physical section supplies both
    // passes. Exercise the UnitModel adapter, not a test-only raster plan.
    let fixture = crate::test_fixture::json("tools/voxel_oracle/aircraft_shadow.json");
    let decode = |key| {
        fixture["generated"][key]
            .as_str()
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>()
    };
    let directory = crate::map::source::test_support::TestDirectory::new("gpu-tumble-section");
    std::fs::write(directory.path().join("MULTI.VXL"), decode("vxl")).unwrap();
    let files =
        crate::assets::asset_manager::AssetManager::from_loose_root_for_test(directory.path());
    let model = VoxelPart::load(&files, "MULTI", false).unwrap();
    let vpl = VplFile::from_bytes(&decode("vpl")).unwrap();
    let matrix = vxl_raster::projectile_voxel_matrix(
        crate::sim::projectile::ProjectileVelocity::new(3, -2, 1),
    )
    .unwrap();
    let (body, shadow) = crate::render::unit_atlas::UnitModel::render_tumble(
        &model,
        2,
        matrix,
        Some(&vpl),
        false,
        None,
    )
    .unwrap();
    let (paint, stencil) =
        crate::render::unit_atlas::UnitModel::tumble_images(&model, 2, matrix, Some(&vpl), false)
            .unwrap();
    let mut raster = VoxelGpu::default();
    let mut images = Vec::new();
    let mut expected = Vec::new();
    for (image, sprite) in [paint, stencil.unwrap()]
        .into_iter()
        .zip([body, shadow.unwrap()])
    {
        images.push((
            raster
                .admit(&image, Some(&vpl))
                .expect("selected section GPU draw"),
            crop(&sprite),
        ));
        expected.push(sprite.palette_indices);
    }
    assert_eq!(
        draw(&gpu, &batch, &mut raster, &images, Some(&vpl)),
        expected
    );
    let (buffered, _) =
        crate::render::unit_atlas::UnitModel::tumble_images(&model, 2, matrix, Some(&vpl), true)
            .unwrap();
    assert!(
        raster.admit(&buffered, Some(&vpl)).is_none(),
        "UseBuffer stays on the shared CPU depth path"
    );
    let buffered = buffered.render_cpu(Some(&vpl)).unwrap();
    let (cpu, _) = crate::render::unit_atlas::UnitModel::render_tumble(
        &model,
        2,
        matrix,
        Some(&vpl),
        true,
        None,
    )
    .unwrap();
    assert_eq!(buffered.palette_indices, cpu.palette_indices);
}
