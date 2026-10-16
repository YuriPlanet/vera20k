//! Exact ordinary, unmirrored VXL draw preparation and encoded-span raster.
//!
//! Original 7540F0/754510/756590 own corners, shared center, crop and integer
//! strides. This owner is separate from voxel shadow/model-to-world geometry.

use super::{HvaFile, Mat4, Vec3, VxlFile};
use crate::util::native_affine::{ftol, load, matrix_product, store, transform_point};
use crate::util::native_x87::X87Chop53 as Fpu;

// Original 8468C0: start, X edge, Y edge, Z edge, start-X, start-Y,
// column-X step, column-Y step. Rows 4..7 dispatch backward encoded Z.
const ORIENTATIONS: [[i32; 8]; 8] = [
    [0, 3, 1, 4, 1, 1, -1, -1],
    [1, 2, 0, 5, 1, 0, -1, 1],
    [2, 1, 3, 6, 0, 0, 1, 1],
    [3, 0, 2, 7, 0, 1, 1, -1],
    [4, 7, 5, 0, 1, 1, -1, -1],
    [5, 6, 4, 1, 1, 0, -1, 1],
    [6, 5, 7, 2, 0, 0, 1, 1],
    [7, 4, 6, 3, 0, 1, 1, -1],
];

#[derive(Debug, Clone)]
pub(super) struct RasterParams {
    pub column_steps: [i32; 3],
    pub origin_xy: [u16; 2],
    pub axis_xy: [[i16; 2]; 3],
    pub sizes: [u8; 3],
    pub reverse: bool,
}

#[derive(Debug)]
pub(super) struct Section {
    #[cfg(test)]
    pub matrix: Mat4,
    #[cfg(test)]
    pub hva_matrix: Option<Mat4>,
    pub corners: [Vec3; 8],
    #[cfg(test)]
    pub minimum: Vec3,
    pub maximum: Vec3,
    pub corner_index: usize,
    pub params: RasterParams,
}

#[derive(Debug)]
pub(super) struct Geometry {
    pub sections: Vec<Section>,
    pub paint_order: Vec<usize>,
    /// Native world offset X/Y, fixed-buffer crop X/Y, width/height.
    pub rect: [i32; 6],
}

pub(super) fn prepare_geometry(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    frame: u32,
    draw_matrix: Mat4,
) -> Option<Geometry> {
    let scale = vxl.limbs.first()?.scale;
    let mut sections = Vec::with_capacity(vxl.limbs.len());
    let mut minimum = Vec3::splat(10000.0);
    let mut maximum = Vec3::splat(-10000.0);
    for (index, limb) in vxl.limbs.iter().enumerate() {
        let sizes = [limb.size_x, limb.size_y, limb.size_z];
        if sizes.contains(&0) {
            return None;
        }
        let hva_matrix = if let Some(hva) = hva {
            let mut raw = *hva.get_transform(frame, index as u32)?;
            // 5F823E scales the entire HVA with limb zero's scale once.
            for column in [3, 7, 11] {
                raw[column] = store(Fpu::mul(load(raw[column])?, load(scale)?))?;
            }
            Some(super::hva_to_mat4(&raw, 1.0))
        } else {
            None
        };
        let model = match hva_matrix {
            Some(hva) => matrix_product(draw_matrix, hva)?,
            None => draw_matrix, // 706F64 bypasses HVA; tailer transform is not applied.
        };
        let matrix = matrix_product(Mat4::IDENTITY, model)?;
        let [lo_x, lo_y, lo_z, hi_x, hi_y, hi_z] = limb.bounds;
        let raw_corners = [
            Vec3::new(hi_x, hi_y, lo_z),
            Vec3::new(hi_x, lo_y, lo_z),
            Vec3::new(lo_x, lo_y, lo_z),
            Vec3::new(lo_x, hi_y, lo_z),
            Vec3::new(hi_x, hi_y, hi_z),
            Vec3::new(hi_x, lo_y, hi_z),
            Vec3::new(lo_x, lo_y, hi_z),
            Vec3::new(lo_x, hi_y, hi_z),
        ];
        let mut corners = [Vec3::ZERO; 8];
        let mut lo = Vec3::splat(10000.0);
        let mut hi = Vec3::splat(-10000.0);
        let mut corner_index = 0;
        for (i, corner) in raw_corners.into_iter().enumerate() {
            let mut point = transform_point(matrix, corner)?;
            point.y = -point.y;
            if point.z < lo.z {
                corner_index = i;
            }
            for axis in 0..3 {
                // Original comparisons replace only strict extrema; equal
                // values preserve the first stored bits, including signed 0.
                if point[axis] < lo[axis] {
                    lo[axis] = point[axis];
                }
                if point[axis] > hi[axis] {
                    hi[axis] = point[axis];
                }
            }
            corners[i] = point;
        }
        for axis in 0..3 {
            if lo[axis] < minimum[axis] {
                minimum[axis] = lo[axis];
            }
            if hi[axis] > maximum[axis] {
                maximum[axis] = hi[axis];
            }
        }
        sections.push(Section {
            #[cfg(test)]
            matrix,
            #[cfg(test)]
            hva_matrix,
            corners,
            #[cfg(test)]
            minimum: lo,
            maximum: hi,
            corner_index,
            params: RasterParams {
                column_steps: [0; 3],
                origin_xy: [0; 2],
                axis_xy: [[0; 2]; 3],
                sizes,
                reverse: corner_index >= 4,
            },
        });
    }
    let mut center = Vec3::ZERO;
    let mut rect = [0i32; 6];
    for axis in 0..3 {
        center[axis] = store(Fpu::mul(
            Fpu::add(load(minimum[axis])?, load(maximum[axis])?),
            load(0.5)?,
        ))?;
        if axis < 2 {
            let extent = ftol(Fpu::sub(load(maximum[axis])?, load(minimum[axis])?))?;
            rect[axis + 4] = extent.checked_add(8)?;
            rect[axis + 2] = 124 - extent / 2;
            rect[axis] = ftol(load(center[axis])?)? - rect[axis + 4] / 2;
        }
    }
    for section in &mut sections {
        let table = ORIENTATIONS[section.corner_index];
        let origin = section.corners[table[0] as usize];
        let [nx, ny, _nz] = section.params.sizes.map(i32::from);
        section.params.column_steps = [
            table[4] * (nx - 1) + table[5] * (ny - 1) * nx,
            table[6],
            table[7] * nx,
        ];
        for axis in 0..2 {
            let centered = Fpu::sub(
                Fpu::add(load(origin[axis])?, load(128.0)?),
                load(center[axis])?,
            );
            section.params.origin_xy[axis] = ftol(Fpu::mul(centered, load(256.0)?))? as u16;
            for grid_axis in 0..3 {
                let edge = section.corners[table[grid_axis + 1] as usize];
                let difference = Fpu::sub(load(edge[axis])?, load(origin[axis])?);
                let step = Fpu::div(
                    difference,
                    Fpu::load_i32(i32::from(section.params.sizes[grid_axis])),
                )
                .ok()?;
                section.params.axis_xy[grid_axis][axis] =
                    ftol(Fpu::mul(step, load(256.0)?))? as i16;
            }
        }
    }
    let mut paint_order: Vec<usize> = (0..sections.len()).collect();
    // 7545B1 pairwise exchange, deliberately not a stable depth sort.
    for i in 0..paint_order.len() {
        for j in i + 1..paint_order.len() {
            if sections[paint_order[i]].maximum.z > sections[paint_order[j]].maximum.z {
                paint_order.swap(i, j);
            }
        }
    }
    Some(Geometry {
        sections,
        paint_order,
        rect,
    })
}

/// One literal visibility-map store. Order, including stores of zero, decides
/// the winner; palette values and VERA's auxiliary depth never decide it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PaintWrite {
    pub address: u16,
    pub color: u8,
    /// VERA-internal auxiliary depth for existing independently baked part
    /// composition. The native opaque leaf writes no per-pixel Z value.
    pub depth: f32,
}

pub(crate) struct PreparedDraw {
    pub writes: Vec<PaintWrite>,
    pub rect: [i32; 6],
}

fn pair(x: i32, y: i32) -> u32 {
    u32::from(x as u16) | (u32::from(y as u16) << 16)
}

fn column_offset(body: &[u8], table: usize, column: usize) -> Option<i32> {
    let offset = table.checked_add(column.checked_mul(4)?)?;
    Some(i32::from_le_bytes(
        body.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Execute 7DF9C0/7DFAE0's integer addressing over the original run stream.
/// Every read is bounded for malformed external assets; successful native
/// columns consume exactly size_z cells. Every run advances its byte pointer
/// even when skip/count are zero; checked reads bound such streams without
/// rejecting native-accepted empty runs based on an invented run-count cap.
fn paint_section(
    limb: &super::VxlLimb,
    section: &Section,
    pages: &[u8; 256],
    vpl: Option<&super::VplFile>,
    writes: &mut Vec<PaintWrite>,
) -> Option<()> {
    let spans = limb.native_spans.as_ref()?;
    let body = &spans.body;
    let params = &section.params;
    let [nx, ny, nz] = params.sizes.map(usize::from);
    let origin = pair(
        i32::from(params.origin_xy[0]),
        i32::from(params.origin_xy[1]),
    );
    let strides = params
        .axis_xy
        .map(|[x, y]| pair(i32::from(x), i32::from(y)));
    let [dx, dy] = params.axis_xy[2].map(i32::from);
    let orientation = ORIENTATIONS[section.corner_index];
    let corner = section.corners[orientation[0] as usize];
    // Auxiliary VERA depth is attached to the winning native paint point.
    // It is not used for intra-VXL visibility and does not feed row-Z metadata.
    let depth_steps: [f32; 3] = std::array::from_fn(|axis| {
        (section.corners[orientation[axis + 1] as usize].z - corner.z)
            / f32::from(params.sizes[axis])
    });
    for y in 0..ny {
        for x in 0..nx {
            let column = params.column_steps[0]
                + y as i32 * params.column_steps[2]
                + x as i32 * params.column_steps[1];
            let column = usize::try_from(column).ok()?;
            if column >= nx * ny {
                return None;
            }
            let table = if params.reverse {
                spans.end_offset
            } else {
                spans.start_offset
            };
            let offset = column_offset(body, table, column)?;
            if offset < 0 {
                continue;
            }
            let mut pos = spans.data_offset.checked_add(offset as usize)?;
            let mut point = origin
                .wrapping_add(strides[1].wrapping_mul(y as u32))
                .wrapping_add(strides[0].wrapping_mul(x as u32));
            let mut z = 0usize;
            while z < nz {
                let (skip, count) = if params.reverse {
                    let count = usize::from(*body.get(pos)?);
                    pos = pos.checked_sub(1)?;
                    (0, count)
                } else {
                    let skip = usize::from(*body.get(pos)?);
                    let count = usize::from(*body.get(pos.checked_add(1)?)?);
                    pos = pos.checked_add(2)?;
                    (skip, count)
                };
                z = z.checked_add(skip)?;
                if z.checked_add(count)? > nz {
                    return None;
                }
                // Skip products truncate X and Y independently before their
                // packed dword is added. Occupied strides below retain carry.
                point = point.wrapping_add(pair(skip as i32 * dx, skip as i32 * dy));
                for _ in 0..count {
                    let (color, normal) = if params.reverse {
                        let normal = *body.get(pos)?;
                        let color = *body.get(pos.checked_sub(1)?)?;
                        pos = pos.checked_sub(2)?;
                        (color, normal)
                    } else {
                        let color = *body.get(pos)?;
                        let normal = *body.get(pos.checked_add(1)?)?;
                        pos = pos.checked_add(2)?;
                        (color, normal)
                    };
                    let color = vpl.map_or(color, |vpl| {
                        vpl.get_palette_index(pages[usize::from(normal)], color)
                    });
                    let address = ((point >> 8) & 0xff) | ((point >> 16) & 0xff00);
                    writes.push(PaintWrite {
                        address: address as u16,
                        color,
                        depth: corner.z
                            + x as f32 * depth_steps[0]
                            + y as f32 * depth_steps[1]
                            + z as f32 * depth_steps[2],
                    });
                    point = point.wrapping_add(strides[2]);
                    z += 1;
                }
                if params.reverse {
                    // The pointer now names the leading count. Native skips
                    // it and consumes the preceding skip byte backward.
                    pos = pos.checked_sub(1)?;
                    let skip = usize::from(*body.get(pos)?);
                    // Native decrements even after the last run; the value is
                    // then unused. Saturation permits a stream at body start.
                    pos = pos.saturating_sub(1);
                    z = z.checked_add(skip)?;
                    if z > nz {
                        return None;
                    }
                    point = point.wrapping_add(pair(skip as i32 * dx, skip as i32 * dy));
                } else {
                    // The forward leaf ignores the duplicate-count value;
                    // the reverse leaf consumes that byte as its run count.
                    body.get(pos)?;
                    pos = pos.checked_add(1)?;
                }
            }
        }
    }
    Some(())
}

pub(super) fn prepare_draw(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &super::VxlRenderParams,
    vpl: Option<&super::VplFile>,
) -> Option<PreparedDraw> {
    if params.scale != 1.0 || vxl.limbs.iter().any(|limb| limb.native_spans.is_none()) {
        return None;
    }
    let draw_matrix =
        super::voxel_params_draw_rotation(params, super::voxel_facing_step(params.facing));
    let geometry = prepare_geometry(vxl, hva, params.frame, draw_matrix)?;
    // 706ED0 calls 753D00 once for the VXL, before submitting its sections;
    // VoxelAnimClass::DrawIt calls 753C80 instead (749DA3). Either reads the
    // normal mode from limb zero, not switched for every limb.
    let normals_mode = vxl.limbs.first()?.normals_mode;
    let rotation = glam::Mat3::from_mat4(draw_matrix);
    let pages = params.lighting.pages(normals_mode, rotation);
    let mut writes = Vec::with_capacity(vxl.limbs.iter().map(|limb| limb.voxels.len()).sum());
    for &index in &geometry.paint_order {
        paint_section(
            &vxl.limbs[index],
            &geometry.sections[index],
            &pages,
            vpl,
            &mut writes,
        )?;
    }
    Some(PreparedDraw {
        writes,
        rect: geometry.rect,
    })
}

impl PreparedDraw {
    /// Extract the native crop after completing the fixed 256x256 draw. A
    /// large/malformed crop is rejected; addressing inside the draw wraps as
    /// the original does, while reading outside its surface is never allowed.
    pub(crate) fn crop_indices(&self, visibility: &[u8]) -> Option<Vec<u8>> {
        let [_, _, x, y, width, height] = self.rect;
        if visibility.len() != 65536
            || x < 0
            || y < 0
            || width <= 0
            || height <= 0
            || x.checked_add(width)? > 256
            || y.checked_add(height)? > 256
        {
            return None;
        }
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for row in y..y + height {
            let start = (row * 256 + x) as usize;
            pixels.extend_from_slice(&visibility[start..start + width as usize]);
        }
        Some(pixels)
    }

    pub(crate) fn render_cpu(&self) -> Option<super::VxlSprite> {
        let mut visibility = vec![0u8; 65536];
        let mut depth = vec![f32::NEG_INFINITY; 65536];
        for write in &self.writes {
            let index = usize::from(write.address);
            visibility[index] = write.color;
            depth[index] = if write.color == 0 {
                f32::NEG_INFINITY
            } else {
                write.depth
            };
        }
        let palette_indices = self.crop_indices(&visibility)?;
        let [offset_x, offset_y, x, y, width, height] = self.rect;
        let mut cropped_depth = Vec::with_capacity(palette_indices.len());
        for row in y..y + height {
            let start = (row * 256 + x) as usize;
            cropped_depth.extend_from_slice(&depth[start..start + width as usize]);
        }
        Some(super::VxlSprite {
            palette_indices,
            depth: cropped_depth,
            width: width as u32,
            height: height as u32,
            offset_x: offset_x as f32,
            offset_y: offset_y as f32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn words(value: &Value) -> Vec<u32> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_u64().unwrap() as u32)
            .collect()
    }

    fn matrix_bits(matrix: Mat4) -> Vec<u32> {
        (0..3)
            .flat_map(|row| (0..4).map(move |col| matrix.col(col)[row].to_bits()))
            .collect()
    }

    /// One native Render case through the production geometry, paint and
    /// crop. Returns its backward raster dispatches and the zero results that
    /// erased an earlier visible pixel.
    fn compare_native_render(
        vxl: &VxlFile,
        hva: Option<&HvaFile>,
        vpl: &super::super::VplFile,
        params: &super::super::VxlRenderParams,
        matrix: Mat4,
        case: &Value,
        label: &str,
    ) -> (usize, usize) {
        let mut backward = 0;
        assert_eq!(
            matrix_bits(matrix),
            words(&case["input_matrix_bits"]),
            "{label} production draw matrix"
        );
        let geometry = prepare_geometry(vxl, hva, 0, matrix).unwrap();
        let expected_rect: Vec<i32> = case["rect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(
            geometry.rect.as_slice(),
            expected_rect,
            "{label} all six crop outputs"
        );
        assert_eq!(
            geometry
                .paint_order
                .iter()
                .map(|i| *i as u32)
                .collect::<Vec<_>>(),
            words(&case["paint_order"]),
            "{label} section exchange order"
        );
        for (index, section) in geometry.sections.iter().enumerate() {
            assert_eq!(
                matrix_bits(section.matrix),
                words(&case["section_matrix_bits"][index]),
                "{label} section {index} matrix"
            );
            if let Some(hva_matrix) = section.hva_matrix {
                assert_eq!(
                    matrix_bits(hva_matrix),
                    words(&case["hva_matrix_bits"][index]),
                    "{label} section {index} globally scaled HVA"
                );
            } else {
                assert!(case["hva_matrix_bits"].as_array().unwrap().is_empty());
            }
            let expected = &case["boxes"][index];
            assert_eq!(
                section.corner_index as u64,
                expected["corner_index"].as_u64().unwrap(),
                "{label} section {index} orientation"
            );
            let extents = [section.minimum, section.maximum]
                .into_iter()
                .flat_map(|v| v.to_array().map(f32::to_bits))
                .collect::<Vec<_>>();
            assert_eq!(
                extents,
                words(&expected["extents_bits"]),
                "{label} section {index} bounds bits"
            );
            let corners = section
                .corners
                .into_iter()
                .flat_map(|v| v.to_array().map(f32::to_bits))
                .collect::<Vec<_>>();
            assert_eq!(
                corners,
                words(&expected["corner_bits"]),
                "{label} section {index} corner bits"
            );
        }
        for (order, &index) in geometry.paint_order.iter().enumerate() {
            let params = &geometry.sections[index].params;
            let actual = serde_json::json!({
                "entry": if params.reverse { 0x7dfae0 } else { 0x7df9c0 },
                "column_steps": params.column_steps, "origin_xy": params.origin_xy,
                "axis_xy": params.axis_xy, "sizes": params.sizes,
            });
            assert_eq!(
                actual, case["raster_params"][order],
                "{label} section {index} live integer parameters"
            );
            backward += usize::from(params.reverse);
        }
        let draw = prepare_draw(vxl, hva, params, Some(vpl)).unwrap();
        let mut visibility = vec![0u8; 65536];
        let mut zero = 0;
        let mut erased = 0;
        for write in &draw.writes {
            let address = usize::from(write.address);
            if write.color == 0 {
                zero += 1;
                erased += usize::from(visibility[address] != 0);
            }
            visibility[address] = write.color;
        }
        assert_eq!(
            serde_json::json!({"all": draw.writes.len(), "zero": zero, "zero_erases_nonzero": erased}),
            case["write_counts"],
            "{label} all paint events"
        );

        let mut expected = vec![0u8; 65536];
        for pixel in case["pixels"].as_array().unwrap() {
            expected[pixel[0].as_u64().unwrap() as usize] = pixel[1].as_u64().unwrap() as u8;
        }
        assert_eq!(
            visibility, expected,
            "{label} complete native visibility buffer"
        );
        let sprite = super::super::render_vxl(vxl, hva, params, Some(vpl));
        assert_eq!(
            sprite.palette_indices,
            draw.crop_indices(&expected).unwrap(),
            "{label} production CPU crop"
        );
        assert_eq!(
            [
                sprite.offset_x as i32,
                sprite.offset_y as i32,
                sprite.width as i32,
                sprite.height as i32
            ],
            [
                geometry.rect[0],
                geometry.rect[1],
                geometry.rect[4],
                geometry.rect[5]
            ],
            "{label} production sprite metadata"
        );
        assert_eq!(
            super::super::native_vxl_draw_bounds(vxl, hva, params),
            Some([
                geometry.rect[0],
                geometry.rect[1],
                geometry.rect[4],
                geometry.rect[5]
            ]),
            "{label} shared row-Z metadata authority"
        );
        (backward, erased)
    }

    /// Actual Bullet468090 ->46B0C0 ->754510 execution, including its
    /// velocity conversion, HVA frame0, diffuse pages and encoded span writes.
    #[test]
    fn kirov_bullet_pose_and_raster_match_original_execution() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/bullet_fixtures.json",
        ))
        .unwrap();
        // Honor the required-retail INI contract as well as archive admission.
        // The corpus contains derived native output and asset identities only;
        // copyrighted VXL/HVA/VPL input bytes come from the local install.
        let Some(_battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, local_assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let assets = &fixture["assets"];
        let asset_bytes = |kind: &str| {
            local_assets
                .get_ref(assets[kind]["name"].as_str().unwrap())
                .unwrap()
        };
        for (name, asset) in assets.as_object().unwrap() {
            let data = asset_bytes(name);
            assert_eq!(data.len() as u64, asset["bytes"], "{name} length");
            assert_eq!(
                crate::util::sha256::sha256_hex(data),
                asset["sha256"],
                "{name} hash"
            );
        }
        let vxl = VxlFile::from_bytes(asset_bytes("vxl")).unwrap();
        let hva = HvaFile::from_bytes(asset_bytes("hva")).unwrap();
        let vpl = super::super::VplFile::from_bytes(asset_bytes("vpl")).unwrap();
        let controls = fixture["controls"].as_array().unwrap();
        assert_eq!(controls.len(), 22);
        for row in controls {
            let label = row["name"].as_str().unwrap();
            let velocity =
                crate::sim::projectile::ProjectileVelocity::from_native(std::array::from_fn(|i| {
                    crate::util::native_x87::NativeF64Bits::from_bits(
                        u64::from_str_radix(row["velocity_bits"][i].as_str().unwrap(), 16).unwrap(),
                    )
                }));
            let [heading, pitch] = super::super::projectile_voxel_angles(velocity).unwrap();
            assert_eq!(u64::from(heading), row["heading_word"], "{label} heading");
            assert_eq!(u64::from(pitch), row["pitch_word"], "{label} pitch");
            assert_eq!(
                u64::from(super::super::voxel_facing_step_u16(heading)),
                row["heading_step"],
                "{label}"
            );
            assert_eq!(
                u64::from(super::super::voxel_facing_step_u16(pitch)),
                row["pitch_step"],
                "{label}"
            );
            let matrix = super::super::projectile_voxel_matrix(velocity).unwrap();
            assert_eq!(matrix_bits(matrix), words(&row["matrix_bits"]), "{label}");
            let params = super::super::VxlRenderParams {
                body_tilt: Some(super::super::BodyTilt::Matrix(matrix)),
                lighting: super::super::VxlLighting::Diffuse,
                ..Default::default()
            };
            assert_eq!(
                params
                    .lighting
                    .pages(vxl.limbs[0].normals_mode, glam::Mat3::from_mat4(matrix))
                    .as_slice(),
                bytes(row["body"]["normal_pages"].as_str().unwrap()),
                "{label} diffuse"
            );
            compare_native_render(&vxl, Some(&hva), &vpl, &params, matrix, &row["body"], label);
        }
    }

    #[test]
    fn native_sections_match_all_geometry_and_visibility_bytes() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/raster_fixtures.json",
        ))
        .unwrap();
        let mut compared = 0;
        let mut backward = 0;
        let mut zero_erases = 0;
        for model in fixture["models"].as_array().unwrap() {
            let vxl = VxlFile::from_bytes(&bytes(model["vxl"].as_str().unwrap())).unwrap();
            let hva = model["hva"]
                .as_str()
                .map(|hex| HvaFile::from_bytes(&bytes(hex)).unwrap());
            let vpl =
                super::super::VplFile::from_bytes(&bytes(model["vpl"].as_str().unwrap())).unwrap();
            for case in model["cases"].as_array().unwrap() {
                let step = case["step"].as_u64().unwrap() as u8;
                let label = format!("{} step {step}", model["name"].as_str().unwrap());
                let params = super::super::VxlRenderParams {
                    facing: step * 8,
                    ..Default::default()
                };
                let matrix = super::super::voxel_draw_rotation_for_state(0, None, step);
                assert_eq!(
                    super::super::vxl_normals::blinn_phong_pages(
                        vxl.limbs[0].normals_mode,
                        glam::Mat3::from_mat4(matrix)
                    )
                    .as_slice(),
                    bytes(case["normal_pages"].as_str().unwrap()),
                    "{label} native normal pages"
                );
                let (reversed, erased) =
                    compare_native_render(&vxl, hva.as_ref(), &vpl, &params, matrix, case, &label);
                backward += reversed;
                zero_erases += erased;
                compared += 1;
            }
        }
        assert_eq!(compared, 32);
        assert_eq!(backward, 20);
        assert_eq!(zero_erases, 1729);
    }

    /// `VoxelAnimClass::DrawIt`'s body (`0x00749D73..0x00749DD4`): the bounce
    /// drawing matrix under the identity viewer, no HVA, lit through 753C80.
    #[test]
    fn tumbling_body_matches_original_execution() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/tumble_fixtures.json",
        ))
        .unwrap();
        let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
        let vpl =
            super::super::VplFile::from_bytes(&bytes(fixture["vpl"].as_str().unwrap())).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        let mut backward = 0;
        for case in cases {
            let body = &case["body"];
            let label = format!("flight {} tick {}", case["flight"], case["tick"]);
            let bits = words(&body["input_matrix_bits"]);
            let matrix =
                super::super::hva_to_mat4(&std::array::from_fn(|i| f32::from_bits(bits[i])), 1.0);
            assert_eq!(
                super::super::vxl_normals::diffuse_pages(
                    vxl.limbs[0].normals_mode,
                    glam::Mat3::from_mat4(matrix)
                )
                .as_slice(),
                bytes(body["normal_pages"].as_str().unwrap()),
                "{label} native diffuse pages"
            );
            let params = super::super::VxlRenderParams {
                body_tilt: Some(super::super::BodyTilt::Matrix(matrix)),
                lighting: super::super::VxlLighting::Diffuse,
                ..Default::default()
            };
            backward += compare_native_render(&vxl, None, &vpl, &params, matrix, body, &label).0;
        }
        assert_eq!(cases.len(), 24);
        assert_eq!(backward, 7);
    }
}
