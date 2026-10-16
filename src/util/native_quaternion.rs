//! Shared original quaternion drawing transform.
//!
//! The arithmetic owner is shared by debris orientation and terrain slope
//! interpolation; neither consumer rebuilds the other's state.

use crate::util::native_x87::{NativeF32Bits, NativeX87Error, X87Chop53, X87Value};

fn load_all<const N: usize>(bits: [NativeF32Bits; N]) -> Result<[X87Value; N], NativeX87Error> {
    let mut values = [X87Chop53::load_i32(0); N];
    for (value, bits) in values.iter_mut().zip(bits) {
        *value = X87Chop53::load_f32(bits)?;
    }
    Ok(values)
}

fn stored(value: X87Value) -> Result<X87Value, NativeX87Error> {
    X87Chop53::load_f32(X87Chop53::store_f32(value)?)
}

/// `Quaternion_ToMatrix @ 0x00646980`, rows `[m0, m1, m2, 0]`. Some products
/// are stored as floats before their sums and some stay on the stack, which
/// is why the rows are not written symmetrically.
pub(crate) fn quaternion_to_matrix(
    q: [NativeF32Bits; 4],
) -> Result<[NativeF32Bits; 12], NativeX87Error> {
    let [x, y, z, w] = load_all(q)?;
    let mul = X87Chop53::mul;
    let one = X87Chop53::load_f32(NativeF32Bits::ONE)?;
    let double = |value: X87Value| X87Chop53::add(value, value);
    let zz = mul(z, z);
    let yy = stored(mul(y, y))?;
    let xx = stored(mul(x, x))?;
    let yx = mul(y, x);
    let wz = mul(w, z);
    let wy = stored(mul(w, y))?;
    let xz = mul(x, z);
    let yz = mul(y, z);
    let wx = mul(w, x);
    let zero = NativeF32Bits::POSITIVE_ZERO;
    let entry = X87Chop53::store_f32;
    Ok([
        entry(X87Chop53::sub(one, double(X87Chop53::add(yy, zz))))?,
        entry(double(X87Chop53::sub(yx, wz)))?,
        entry(double(X87Chop53::add(xz, wy)))?,
        zero,
        entry(double(X87Chop53::add(wz, yx)))?,
        entry(X87Chop53::sub(one, double(X87Chop53::add(xx, zz))))?,
        entry(double(X87Chop53::sub(yz, wx)))?,
        zero,
        entry(double(X87Chop53::sub(stored(xz)?, wy)))?,
        entry(double(X87Chop53::add(wx, yz)))?,
        entry(X87Chop53::sub(one, double(X87Chop53::add(xx, yy))))?,
        zero,
    ])
}
