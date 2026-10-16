//! Original affine matrix primitives shared by drawing and GetFLH.
//! Float stores are explicit; these functions do not apply camera, palette,
//! screen projection or simulation state mutations.
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as Fpu, X87Value};
use glam::{Mat4, Vec3};

pub fn load(value: f32) -> Option<X87Value> {
    Fpu::load_f32(NativeF32Bits::from_bits(value.to_bits())).ok()
}

pub fn store(value: X87Value) -> Option<f32> {
    Some(f32::from_bits(Fpu::store_f32(value).ok()?.bits()))
}

pub fn ftol(value: X87Value) -> Option<i32> {
    i32::try_from(Fpu::ftol_i64(value).ok()?).ok()
}

/// Full affine 5AF980, including its translation-column float stores.
pub fn matrix_product(left: Mat4, right: Mat4) -> Option<Mat4> {
    let mut result = Mat4::IDENTITY;
    for col in 0..4 {
        for row in 0..3 {
            let z = Fpu::mul(load(left.col(2)[row])?, load(right.col(col)[2])?);
            let y = Fpu::mul(load(left.col(1)[row])?, load(right.col(col)[1])?);
            let x = Fpu::mul(load(left.col(0)[row])?, load(right.col(col)[0])?);
            let mut sum = Fpu::add(Fpu::add(z, y), x);
            if col == 3 {
                sum = Fpu::add(sum, load(left.col(3)[row])?);
            }
            result.col_mut(col)[row] = store(sum)?;
        }
    }
    Some(result)
}

/// 5AFB80 differs from matrix multiplication in the second/third dot order.
pub fn transform_point(matrix: Mat4, point: Vec3) -> Option<Vec3> {
    let mut result = Vec3::ZERO;
    for row in 0..3 {
        let x = Fpu::mul(load(matrix.col(0)[row])?, load(point.x)?);
        let y = Fpu::mul(load(matrix.col(1)[row])?, load(point.y)?);
        let z = Fpu::mul(load(matrix.col(2)[row])?, load(point.z)?);
        let sum = if row == 0 {
            Fpu::add(Fpu::add(z, y), x)
        } else {
            Fpu::add(Fpu::add(x, z), y)
        };
        result[row] = store(Fpu::add(sum, load(matrix.col(3)[row])?))?;
    }
    Some(result)
}

/// `Matrix_rotate_y_axis @ 0x005AF080`: post-multiplies a 3x4 matrix in
/// place by a rotation about its Y axis. Each row's X and Z become
/// `x*cos - z*sin` and `z*cos + x*sin`, evaluated in x87 precision and
/// stored as floats; Y and the translation column are untouched. The sine
/// and cosine are the table's (`0x004CACB0`, `0x004CAD00`).
pub fn rotate_y(matrix: Mat4, sin: f32, cos: f32) -> Mat4 {
    use crate::util::native_x87::{NativeF32Bits, X87Chop53 as Fpu};
    let load = |value: f32| {
        Fpu::load_f32(NativeF32Bits::from_bits(value.to_bits())).expect("voxel basis is finite")
    };
    let store =
        |value| f32::from_bits(Fpu::store_f32(value).expect("rotation fits a float").bits());
    let mut result = matrix;
    for row in 0..3 {
        let x = load(matrix.col(0)[row]);
        let z = load(matrix.col(2)[row]);
        result.col_mut(0)[row] = store(Fpu::sub(Fpu::mul(x, load(cos)), Fpu::mul(z, load(sin))));
        result.col_mut(2)[row] = store(Fpu::add(Fpu::mul(z, load(cos)), Fpu::mul(x, load(sin))));
    }
    result
}

/// Native in-place axis rotation (5AEF60/5AF080/5AF1A0): each row's two
/// changed columns store separately as floats. Reuse the existing Y owner.
pub fn rotate_x(matrix: Mat4, sin: f32, cos: f32) -> Mat4 {
    use crate::util::native_x87::X87Chop53 as Fpu;
    let mut result = matrix;
    for row in 0..3 {
        let y = load(matrix.col(1)[row]).expect("finite voxel basis");
        let z = load(matrix.col(2)[row]).expect("finite voxel basis");
        let sin = load(sin).expect("finite table entry");
        let cos = load(cos).expect("finite table entry");
        result.col_mut(1)[row] = store(Fpu::add(Fpu::mul(y, cos), Fpu::mul(z, sin))).unwrap();
        result.col_mut(2)[row] = store(Fpu::sub(Fpu::mul(z, cos), Fpu::mul(y, sin))).unwrap();
    }
    result
}

pub fn rotate_z(matrix: Mat4, sin: f32, cos: f32) -> Mat4 {
    use crate::util::native_x87::X87Chop53 as Fpu;
    let mut result = matrix;
    for row in 0..3 {
        let x = load(matrix.col(0)[row]).expect("finite voxel basis");
        let y = load(matrix.col(1)[row]).expect("finite voxel basis");
        let sin = load(sin).expect("finite table entry");
        let cos = load(cos).expect("finite table entry");
        result.col_mut(0)[row] = store(Fpu::add(Fpu::mul(x, cos), Fpu::mul(y, sin))).unwrap();
        result.col_mut(1)[row] = store(Fpu::sub(Fpu::mul(y, cos), Fpu::mul(x, sin))).unwrap();
    }
    result
}

/// Original TranslateX5AE980 and Translate5AE8F0 update the translation
/// column from the existing basis. A vector in this frame is not a screen
/// displacement; keep the native float store before the next rotation.
pub fn translate(matrix: Mat4, offset: &[f32]) -> Mat4 {
    use crate::util::native_x87::X87Chop53 as Fpu;
    debug_assert!(offset.len() == 1 || offset.len() == 3);
    let mut result = matrix;
    for row in 0..3 {
        let mut sum = load(matrix.col(3)[row]).expect("finite voxel translation");
        for (axis, &amount) in offset.iter().enumerate() {
            sum = Fpu::add(
                Fpu::mul(load(amount).unwrap(), load(matrix.col(axis)[row]).unwrap()),
                sum,
            );
            // Vector5AE8F0 reloads X/Y float intermediates after each axis,
            // but retains its Z accumulation on the x87 stack to the end.
            if row < 2 && axis + 1 < offset.len() {
                sum = load(store(sum).unwrap()).unwrap();
            }
        }
        result.col_mut(3)[row] = store(sum).expect("translation fits float");
    }
    result
}

/// Three scalar arguments, original `Translate5AE890` used by GetFLH6F3C68
/// and6F3CBA. Unlike vector5AE8F0, this retains each complete dot product
/// before its single float store, and the X row sums Y/Z before X.
/// Keep the scalar and vector entry points distinct: regrouping or their
/// different reloads can move a fired projectile by a whole lepton.
pub(crate) fn translate_xyz(matrix: Mat4, offset: [f32; 3]) -> Mat4 {
    let [x, y, z] = offset.map(|value| load(value).expect("finite native translation"));
    let mut result = matrix;
    for row in 0..3 {
        let x = Fpu::mul(x, load(matrix.col(0)[row]).unwrap());
        let y = Fpu::mul(y, load(matrix.col(1)[row]).unwrap());
        let z = Fpu::mul(z, load(matrix.col(2)[row]).unwrap());
        let dot = if row == 0 {
            Fpu::add(Fpu::add(y, z), x)
        } else {
            Fpu::add(Fpu::add(x, y), z)
        };
        result.col_mut(3)[row] = store(Fpu::add(dot, load(matrix.col(3)[row]).unwrap()))
            .expect("native translation fits float");
    }
    result
}
