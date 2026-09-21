//! RA2 voxel normal vectors and diffuse lighting.
//!
//! RA2 voxel models store a per-voxel `normal_index` (0–255) used for
//! directional lighting. The game ships 245 pre-computed unit normal
//! vectors for normals_mode=4. Tiberian Sun uses a 36-entry table (mode=2).
//!
//! Source: Sleipnir's Stuff forum.
//! Normal coordinates are in VXL model space.
//!
//! ## Dependency rules
//! - Part of render/ — depends on glam and shared native numeric helpers.

use glam::{Mat3, Vec3};

use crate::util::native_x87::{
    NativeF32Bits, X87Chop53 as Fpu, X87Ordering, X87Value, sqrt_approx_f32,
};

/// Number of normals in RA2 mode (normals_mode = 4).
const RA2_NORMAL_COUNT: usize = 245;

/// Number of normals in Tiberian Sun mode (normals_mode = 2).
const TS_NORMAL_COUNT: usize = 36;

/// VPL page stored in normal→page LUT slots 253–255 by the original engine's
/// lighting precompute (its "ambient" constant). Voxels referencing stale
/// normal index 255 — present in 8 retail models — render with this page.
const AMBIENT_PAGE: u8 = 16;

/// 245 RA2 normal vectors, matching the original engine's mode-4 table exactly.
/// Index 240 is the final new vector; indices 241–244 repeat it byte-for-byte,
/// and there is no native entry 245. Retail VXL data DOES reference index
/// 255 — but only inside limbs named DUMMY01/DUMMY02 (1,931 voxels across 8
/// retail files, incl. the slave-miner refinery turret; verified by
/// tests/retail_goldens certify_vxl_structural). Indices 245–254 never occur
/// in retail data.
///
/// The original engine shades index 255 deliberately: its lighting
/// precompute fills the normal→VPL-page LUT only up to the mode's entry
/// count, then stores the ambient constant page 16 into LUT slots
/// 253/254/255 for both modes, and the rasterizer indexes the LUT with the
/// raw unclamped normal byte — so normal 255 always renders with VPL page
/// 16. `blinn_phong_pages` mirrors that tail unconditionally; `get_normal`'s
/// +Z fallback is only a safety for the never-observed 245–252 gap.
/// Full chain: docs/research/VXL_STALE_NORMAL_255_AMBIENT_PAGE_GHIDRA_REPORT.md.
#[rustfmt::skip]
static RA2_NORMALS: [[f32; 3]; RA2_NORMAL_COUNT] = [
    [ 0.526578, -0.359621, -0.770317], [ 0.150482,  0.435984,  0.887284],
    [ 0.414195,  0.738255, -0.532374], [ 0.075152,  0.916249, -0.393498],
    [-0.316149,  0.930736, -0.183793], [-0.773819,  0.623334, -0.112510],
    [-0.900842,  0.428537, -0.069568], [-0.998942, -0.010971,  0.044665],
    [-0.979761, -0.157670, -0.123324], [-0.911274, -0.362371, -0.195620],
    [-0.624069, -0.720941, -0.301301], [-0.310173, -0.809345, -0.498752],
    [ 0.146613, -0.815819, -0.559414], [-0.716516, -0.694356, -0.066888],
    [ 0.503972, -0.114202, -0.856137], [ 0.455491,  0.872627, -0.176211],
    [-0.005010, -0.114373, -0.993425], [-0.104675, -0.327701, -0.938965],
    [ 0.560412,  0.752589, -0.345756], [-0.060576,  0.821628, -0.566796],
    [-0.302341,  0.797007, -0.522847], [-0.671543,  0.670740, -0.314863],
    [-0.778401, -0.128357,  0.614505], [-0.924050,  0.278382, -0.261985],
    [-0.699773, -0.550491, -0.455278], [-0.568248, -0.517189, -0.640008],
    [ 0.054098, -0.932864, -0.356143], [ 0.758382,  0.572893, -0.310888],
    [ 0.003620,  0.305026, -0.952337], [-0.060850, -0.986886, -0.149511],
    [ 0.635230,  0.045478, -0.770983], [ 0.521705,  0.241309, -0.818287],
    [ 0.269404,  0.635425, -0.723641], [ 0.045676,  0.672754, -0.738455],
    [-0.180511,  0.674657, -0.715719], [-0.397131,  0.636640, -0.661042],
    [-0.552004,  0.472515, -0.687038], [-0.772170,  0.083090, -0.629960],
    [-0.669819, -0.119533, -0.732840], [-0.540455, -0.318444, -0.778782],
    [-0.386135, -0.522789, -0.759994], [-0.261466, -0.688567, -0.676395],
    [-0.019412, -0.696103, -0.717680], [ 0.303569, -0.481844, -0.821993],
    [ 0.681939, -0.195129, -0.704900], [-0.244889, -0.116562, -0.962519],
    [ 0.800759, -0.022979, -0.598546], [-0.370275,  0.095584, -0.923991],
    [-0.330671, -0.326578, -0.885440], [-0.163220, -0.527579, -0.833679],
    [ 0.126390, -0.313146, -0.941257], [ 0.349548, -0.272226, -0.896498],
    [ 0.239918, -0.085825, -0.966992], [ 0.390845,  0.081537, -0.916838],
    [ 0.255267,  0.268697, -0.928785], [ 0.146245,  0.480438, -0.864749],
    [-0.326016,  0.478456, -0.815349], [-0.469682, -0.112519, -0.875636],
    [ 0.818440, -0.258520, -0.513151], [-0.474318,  0.292238, -0.830433],
    [ 0.778943,  0.395842, -0.486371], [ 0.624094,  0.393773, -0.674870],
    [ 0.740886,  0.203834, -0.639953], [ 0.480217,  0.565768, -0.670297],
    [ 0.380930,  0.424535, -0.821378], [-0.093422,  0.501124, -0.860318],
    [-0.236485,  0.296198, -0.925387], [-0.131531,  0.093959, -0.986849],
    [-0.823562,  0.295777, -0.484006], [ 0.611066, -0.624304, -0.486664],
    [ 0.069496, -0.520330, -0.851133], [ 0.226522, -0.664879, -0.711775],
    [ 0.471308, -0.568904, -0.673957], [ 0.388425, -0.742624, -0.545560],
    [ 0.783675, -0.480729, -0.393385], [ 0.962394,  0.135676, -0.235349],
    [ 0.876607,  0.172034, -0.449406], [ 0.633405,  0.589793, -0.500941],
    [ 0.182276,  0.800658, -0.570721], [ 0.177003,  0.764134,  0.620297],
    [-0.544016,  0.675515, -0.497721], [-0.679297,  0.286467, -0.675642],
    [-0.590391,  0.091369, -0.801929], [-0.824360, -0.133124, -0.550189],
    [-0.715794, -0.334542, -0.612961], [ 0.174286, -0.892484,  0.416049],
    [-0.082528, -0.837123, -0.540753], [ 0.283331, -0.880874, -0.379189],
    [ 0.675134, -0.426627, -0.601817], [ 0.843720, -0.512335, -0.160156],
    [ 0.977304, -0.098556, -0.187520], [ 0.846295,  0.522672, -0.102947],
    [ 0.677141,  0.721325, -0.145501], [ 0.320965,  0.870892, -0.372194],
    [-0.178978,  0.911533, -0.370236], [-0.447169,  0.826701, -0.341474],
    [-0.703203,  0.496328, -0.509081], [-0.977181,  0.063563, -0.202674],
    [-0.878170, -0.412938,  0.241455], [-0.835831, -0.358550, -0.415728],
    [-0.499174, -0.693433, -0.519592], [-0.188789, -0.923753, -0.333225],
    [ 0.192254, -0.969361, -0.152896], [ 0.515940, -0.783907, -0.345392],
    [ 0.905925, -0.300952, -0.297871], [ 0.991112, -0.127746,  0.037107],
    [ 0.995135,  0.098424, -0.004383], [ 0.760123,  0.646277,  0.067367],
    [ 0.205221,  0.959580, -0.192591], [-0.042750,  0.979513, -0.196791],
    [-0.438017,  0.898927,  0.008492], [-0.821994,  0.480785, -0.305239],
    [-0.899917,  0.081710, -0.428337], [-0.926612, -0.144618, -0.347096],
    [-0.793660, -0.557792, -0.242839], [-0.431350, -0.847779, -0.308558],
    [-0.005492, -0.965000,  0.262193], [ 0.587905, -0.804026, -0.088940],
    [ 0.699493, -0.667686, -0.254765], [ 0.889303,  0.359795, -0.282291],
    [ 0.780972,  0.197037,  0.592672], [ 0.520121,  0.506696,  0.687557],
    [ 0.403895,  0.693961,  0.596060], [-0.154983,  0.899236,  0.409090],
    [-0.657338,  0.537168,  0.528543], [-0.746195,  0.334091,  0.575827],
    [-0.624952, -0.049144,  0.779115], [ 0.318141, -0.254715,  0.913185],
    [-0.555897,  0.405294,  0.725752], [-0.794434,  0.099406,  0.599160],
    [-0.640361, -0.689463,  0.338495], [-0.126713, -0.734095,  0.667120],
    [ 0.105457, -0.780817,  0.615795], [ 0.407993, -0.480916,  0.776055],
    [ 0.695136, -0.545120,  0.468647], [ 0.973191, -0.006489,  0.229908],
    [ 0.946894,  0.317509, -0.050799], [ 0.563583,  0.825612,  0.027183],
    [ 0.325773,  0.945423,  0.006949], [-0.171821,  0.985097, -0.007815],
    [-0.670441,  0.739939,  0.054769], [-0.822981,  0.554962,  0.121322],
    [-0.966193,  0.117857,  0.229307], [-0.953769, -0.294704,  0.058945],
    [-0.864387, -0.502728, -0.010015], [-0.530609, -0.842006, -0.097366],
    [-0.162618, -0.984075,  0.071772], [ 0.081447, -0.996011,  0.036439],
    [ 0.745984, -0.665963,  0.000762], [ 0.942057, -0.329269, -0.064106],
    [ 0.939702, -0.281090,  0.194803], [ 0.771214,  0.550670,  0.319363],
    [ 0.641348,  0.730690,  0.234021], [ 0.080682,  0.996691,  0.009879],
    [-0.046725,  0.976643,  0.209725], [-0.531076,  0.821001,  0.209562],
    [-0.695815,  0.655990,  0.292435], [-0.976122,  0.216709, -0.014913],
    [-0.961661, -0.144129,  0.233314], [-0.772084, -0.613647,  0.165299],
    [-0.449600, -0.836060,  0.314426], [-0.392700, -0.914616,  0.096247],
    [ 0.390589, -0.919470,  0.044890], [ 0.582529, -0.799198,  0.148127],
    [ 0.866431, -0.489812,  0.096864], [ 0.904587,  0.111498,  0.411450],
    [ 0.953537,  0.232330,  0.191806], [ 0.497311,  0.770803,  0.398177],
    [ 0.194066,  0.956320,  0.218611], [ 0.422876,  0.882276,  0.206797],
    [-0.373797,  0.849566,  0.372174], [-0.534497,  0.714023,  0.452200],
    [-0.881827,  0.237160,  0.407598], [-0.904948, -0.014069,  0.425289],
    [-0.751827, -0.512817,  0.414458], [-0.501015, -0.697917,  0.511758],
    [-0.235190, -0.925923,  0.295555], [ 0.228983, -0.953940,  0.193819],
    [ 0.734025, -0.634898,  0.241062], [ 0.913753, -0.063253, -0.401316],
    [ 0.905735, -0.161487,  0.391875], [ 0.858930,  0.342446,  0.380749],
    [ 0.624486,  0.607581,  0.490777], [ 0.289264,  0.857479,  0.425508],
    [ 0.069968,  0.902169,  0.425671], [-0.286180,  0.940700,  0.182165],
    [-0.574013,  0.805119, -0.149309], [ 0.111258,  0.099718, -0.988776],
    [-0.305393, -0.944228, -0.123160], [-0.601166, -0.789576,  0.123163],
    [-0.290645, -0.812140,  0.505919], [-0.064920, -0.877163,  0.475785],
    [ 0.408301, -0.862216,  0.299789], [ 0.566097, -0.725566,  0.391264],
    [ 0.839364, -0.427387,  0.335869], [ 0.818900, -0.041305,  0.572448],
    [ 0.719784,  0.414997,  0.556497], [ 0.881744,  0.450270,  0.140659],
    [ 0.401823, -0.898220, -0.178152], [-0.054020,  0.791344,  0.608980],
    [-0.293774,  0.763994,  0.574465], [-0.450798,  0.610347,  0.651351],
    [-0.638221,  0.186694,  0.746873], [-0.872870, -0.257127,  0.414708],
    [-0.587257, -0.521710,  0.618828], [-0.353658, -0.641974,  0.680291],
    [ 0.041649, -0.611273,  0.790323], [ 0.348342, -0.779183,  0.521087],
    [ 0.499167, -0.622441,  0.602826], [ 0.790019, -0.303831,  0.532500],
    [ 0.660118,  0.060733,  0.748702], [ 0.604921,  0.294161,  0.739960],
    [ 0.385697,  0.379346,  0.841032], [ 0.239693,  0.207876,  0.948332],
    [ 0.012623,  0.258532,  0.965920], [-0.100557,  0.457147,  0.883688],
    [ 0.046967,  0.628588,  0.776319], [-0.430391, -0.445405,  0.785097],
    [-0.434291, -0.196228,  0.879139], [-0.256637, -0.336867,  0.905902],
    [-0.131372, -0.158910,  0.978514], [ 0.102379, -0.208767,  0.972592],
    [ 0.195687, -0.450129,  0.871258], [ 0.627319, -0.423148,  0.653771],
    [ 0.687439, -0.171583,  0.705682], [ 0.275920, -0.021255,  0.960946],
    [ 0.459367,  0.157466,  0.874178], [ 0.285395,  0.583184,  0.760556],
    [-0.812174,  0.460303,  0.358461], [-0.189068,  0.641223,  0.743698],
    [-0.338875,  0.476480,  0.811252], [-0.920994,  0.347186,  0.176727],
    [ 0.040639,  0.024465,  0.998874], [-0.739132, -0.353747,  0.573190],
    [-0.603512, -0.286615,  0.744060], [-0.188676, -0.547059,  0.815554],
    [-0.026045, -0.397820,  0.917094], [ 0.267897, -0.649041,  0.712023],
    [ 0.518246, -0.284891,  0.806386], [ 0.493451, -0.066533,  0.867225],
    // Entry 240 is the final new vector; entries 241–244 repeat it byte-for-byte.
    [-0.328188,  0.140251,  0.934143], [-0.328188,  0.140251,  0.934143],
    [-0.328188,  0.140251,  0.934143], [-0.328188,  0.140251,  0.934143],
    [-0.328188,  0.140251,  0.934143],
];

/// 36 Tiberian Sun normal vectors (normals_mode = 2).
#[rustfmt::skip]
static TS_NORMALS: [[f32; 3]; TS_NORMAL_COUNT] = [
    [ 0.671214,  0.198492, -0.714194], [ 0.269643,  0.584394, -0.765360],
    [-0.040546,  0.096988, -0.994459], [-0.572428, -0.091914, -0.814787],
    [-0.171401, -0.572710, -0.801639], [ 0.362557, -0.302999, -0.881331],
    [ 0.810347, -0.348972, -0.470698], [ 0.103962,  0.938672, -0.328767],
    [-0.324047,  0.587669, -0.741376], [-0.800865,  0.340461, -0.492647],
    [-0.665498, -0.590147, -0.456989], [ 0.314767, -0.803002, -0.506073],
    [ 0.972629,  0.151076, -0.176550], [ 0.680291,  0.684236, -0.262727],
    [-0.520079,  0.827777, -0.210483], [-0.961644, -0.179001, -0.207847],
    [-0.262714, -0.937451, -0.228401], [ 0.219707, -0.971301,  0.091125],
    [ 0.923808, -0.229975,  0.306087], [-0.082489,  0.970660,  0.225866],
    [-0.591798,  0.696790,  0.405289], [-0.925296,  0.366601,  0.097111],
    [-0.705051, -0.687775,  0.172828], [ 0.732400, -0.680367, -0.026305],
    [ 0.855162,  0.374582,  0.358311], [ 0.473006,  0.836480,  0.276705],
    [-0.097617,  0.654112,  0.750072], [-0.904124, -0.153725,  0.398658],
    [-0.211916, -0.858090,  0.467732], [ 0.500227, -0.674408,  0.543091],
    [ 0.584539, -0.110249,  0.803841], [ 0.437373,  0.454644,  0.775889],
    [-0.042441,  0.083318,  0.995619], [-0.596251,  0.220132,  0.772028],
    [-0.506455, -0.396977,  0.765449], [ 0.070569, -0.478474,  0.875262],
];

/// Look up a unit normal vector by normals mode and index.
///
/// Mode 4 (RA2) uses the 245-entry table. Mode 2 (TS) uses the 36-entry table.
/// Out-of-range indices return Vec3::Z (pointing up) as a safe fallback.
#[cfg(test)]
pub fn get_normal(normals_mode: u8, index: u8) -> Vec3 {
    let table: &[[f32; 3]] = match normals_mode {
        4 => &RA2_NORMALS,
        2 => &TS_NORMALS,
        _ => return Vec3::Z,
    };
    let idx: usize = index as usize;
    if idx < table.len() {
        Vec3::new(table[idx][0], table[idx][1], table[idx][2])
    } else {
        Vec3::Z
    }
}

/// Compute diffuse lighting brightness for a voxel.
///
/// Uses N·L (dot product of normal and light direction) for directional
/// diffuse lighting, plus an ambient base level. Returns a brightness
/// multiplier clamped to [0.0, 1.0].
#[cfg(test)]
pub fn diffuse_shade(normal: Vec3, light_dir: Vec3, ambient: f32, diffuse: f32) -> f32 {
    let n_dot_l: f32 = normal.dot(light_dir).max(0.0);
    (ambient + diffuse * n_dot_l).clamp(0.0, 1.0)
}

/// Startup `VXL_LightDirection_Setup @ 0x00754C00`, called at 0x0052BDF5
/// with the original float angle at 0x007E1E68. These are its executed output
/// bits, including native trig-table and x87 rounding; no capture fitting.
/// Source and reproducible vectors: `tools/voxel_oracle/lighting.py`.
const YR_WORLD_LIGHT: Vec3 = Vec3::new(
    f32::from_bits(0xbeff_ffed),
    f32::from_bits(0xbf35_04e6),
    f32::from_bits(0x3eff_ffed),
);

fn fp(value: f32) -> X87Value {
    Fpu::load_f32(NativeF32Bits::from_bits(value.to_bits()))
        .expect("voxel lighting uses finite normal floats")
}

fn fp_store(value: X87Value) -> X87Value {
    Fpu::load_f32(Fpu::store_f32(value).expect("voxel lighting fits a float"))
        .expect("stored lighting float is finite")
}

fn positive(value: X87Value) -> X87Value {
    if Fpu::compare(value, Fpu::load_i32(0)) == X87Ordering::Less {
        Fpu::load_i32(0)
    } else {
        value
    }
}

/// Normal-index -> VPL page table for one ordinary voxel draw.
///
/// `draw_rotation` includes the camera: UnitClass at 0x0073B70E..0x0073B742
/// composes `B44318 * locomotor`, then 0x004DAF10 -> 0x00706640 ->
/// `TechnoClass::Render @ 0x00706ED0` forwards that matrix to
/// `VXL_Init_BlinnPhong @ 0x00753D00`. HVA is composed only afterward.
/// The initializer takes the rigid inverse and transforms the world light;
/// its viewer is +Z because the separate matrix at 0x00887430 is identity.
///
/// The operation order and float stores below follow original 0x005AF4D0
/// and 0x007586F0, including `Sqrt_Approx @ 0x004CAC40`. Native vectors
/// exercise every flat facing in both normal modes, not every rocking pose
/// or the final rasterized image. See `tools/voxel_oracle/lighting.py`.
pub fn blinn_phong_pages(normals_mode: u8, draw_rotation: Mat3) -> [u8; 256] {
    // Native leaves the gap between the mode's table and slot 253 stale.
    // Retail model indices only use the valid table and fixed ambient tail.
    let mut result = [0u8; 256];
    let world = YR_WORLD_LIGHT.to_array().map(fp);
    let light: [X87Value; 3] = std::array::from_fn(|axis| {
        // Rigid inverse transposes the basis. MatrixVectorMultiply evaluates
        // row 0 as (y + z) + x, and rows 1/2 as (y + x) + z.
        let column = draw_rotation.col(axis).to_array().map(fp);
        let products: [X87Value; 3] = std::array::from_fn(|i| Fpu::mul(column[i], world[i]));
        fp_store(if axis == 0 {
            Fpu::add(Fpu::add(products[1], products[2]), products[0])
        } else {
            Fpu::add(Fpu::add(products[1], products[0]), products[2])
        })
    });
    let half_z = Fpu::add(light[2], Fpu::load_i32(1));
    let half = [light[0], light[1], fp_store(half_z)];
    // Z's sum remains on the x87 stack after FST float; X and Y are reloaded.
    let squared = Fpu::add(
        Fpu::add(Fpu::mul(half_z, half[2]), Fpu::mul(half[0], half[0])),
        Fpu::mul(half[1], half[1]),
    );
    let length = Fpu::load_f32(sqrt_approx_f32(squared).expect("finite halfway length"))
        .expect("finite native square root");
    let halfway = if Fpu::compare(length, Fpu::load_i32(0)) == X87Ordering::Equal {
        half
    } else {
        half.map(|value| fp_store(Fpu::div(value, length).expect("nonzero halfway length")))
    };
    let strength = Fpu::load_i32(3); // TechnoClass::Render pushes 0x40400000 at 0x00706F23.
    let table: &[[f32; 3]] = match normals_mode {
        2 => &TS_NORMALS,
        _ => &RA2_NORMALS,
    };
    for (index, normal) in table.iter().enumerate() {
        let normal = normal.map(fp);
        let diffuse = positive(Fpu::add(
            Fpu::add(Fpu::mul(normal[2], light[2]), Fpu::mul(normal[1], light[1])),
            Fpu::mul(normal[0], light[0]),
        ));
        let h = Fpu::add(
            Fpu::add(
                Fpu::mul(halfway[0], normal[0]),
                Fpu::mul(halfway[1], normal[1]),
            ),
            Fpu::mul(halfway[2], normal[2]),
        );
        // Native clamps after the quotient's float store, not before it.
        let denominator = Fpu::add(Fpu::sub(strength, Fpu::mul(h, strength)), h);
        let specular = positive(fp_store(
            Fpu::div(h, denominator).expect("finite Schlick term"),
        ));
        let page = Fpu::mul(Fpu::add(diffuse, specular), Fpu::load_i32(16));
        result[index] = Fpu::ftol_i64(page).expect("voxel page fits an integer") as u8;
    }

    // Ambient tail: the original engine's lighting precompute unconditionally
    // stores page 0x10 into LUT slots 253–255 after filling the mode's table
    // entries, for BOTH normals modes — retail voxels carrying stale normal
    // index 255 (DUMMY limbs) render with this fixed ambient page. Must not
    // be gated on table size: mode 2's table has only 36 entries but its
    // index-255 voxels still get page 16.
    result[253] = AMBIENT_PAGE;
    result[254] = AMBIENT_PAGE;
    result[255] = AMBIENT_PAGE;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ra2_normal_table_has_native_boundary_and_tail() {
        assert_eq!(RA2_NORMALS.len(), 245);
        assert_eq!(RA2_NORMALS[240], [-0.328188, 0.140251, 0.934143]);
        for index in 241..=244 {
            assert_eq!(RA2_NORMALS[index], RA2_NORMALS[240], "index {index}");
        }

        let final_native = Vec3::from_array(RA2_NORMALS[240]);
        assert_eq!(get_normal(4, 244), final_native);
        assert_eq!(get_normal(4, 245), Vec3::Z);

        let pages = blinn_phong_pages(4, Mat3::IDENTITY);
        assert_eq!(pages[244], pages[240]);
        assert_ne!(pages[244], 0, "the final native entry must be processed");
        assert_eq!(&pages[245..253], &[0; 8]);
        assert_eq!(&pages[253..=255], &[AMBIENT_PAGE; 3]);
    }

    #[test]
    fn test_ts_normal_count() {
        for i in 0..36u8 {
            let n: Vec3 = get_normal(2, i);
            let len: f32 = n.length();
            assert!(
                (len - 1.0).abs() < 0.01,
                "TS normal {} has bad length {}",
                i,
                len
            );
        }
        // Out of range falls back to Vec3::Z.
        let fallback: Vec3 = get_normal(2, 36);
        assert_eq!(fallback, Vec3::Z);
    }

    #[test]
    fn stale_normal_255_gets_ambient_page_in_both_modes() {
        // Retail voxels carry normal index 255 in both normals modes (DUMMY
        // limbs); the original engine renders them with the fixed ambient
        // page regardless of mode or facing.
        for facing in [0.0_f32, 1.3, 4.2] {
            for mode in [2u8, 4u8] {
                let pages = blinn_phong_pages(mode, Mat3::from_rotation_z(facing));
                assert_eq!(pages[253], AMBIENT_PAGE, "mode {mode} facing {facing}");
                assert_eq!(pages[254], AMBIENT_PAGE, "mode {mode} facing {facing}");
                assert_eq!(pages[255], AMBIENT_PAGE, "mode {mode} facing {facing}");
            }
        }
    }

    #[test]
    fn native_lighting_initializer_matches_all_flat_facings_and_both_modes() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/voxel_oracle/lighting.json")).unwrap();
        let light: Vec<u32> = vectors["light_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as u32)
            .collect();
        assert_eq!(light, YR_WORLD_LIGHT.to_array().map(f32::to_bits));
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 64);
        for (index, case) in cases.iter().enumerate() {
            assert_eq!(case["step"].as_u64().unwrap(), (index / 2) as u64);
            let mode = case["mode"].as_u64().unwrap() as u8;
            assert_eq!(mode, if index % 2 == 0 { 2 } else { 4 });
            let raw: Vec<f32> = case["draw_matrix_bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| f32::from_bits(value.as_u64().unwrap() as u32))
                .collect();
            let matrix = Mat3::from_cols(
                Vec3::new(raw[0], raw[4], raw[8]),
                Vec3::new(raw[1], raw[5], raw[9]),
                Vec3::new(raw[2], raw[6], raw[10]),
            );
            let expected = case["pages"].as_str().unwrap();
            let actual = blinn_phong_pages(mode, matrix);
            for normal in 0..256 {
                let page = u8::from_str_radix(&expected[normal * 2..normal * 2 + 2], 16).unwrap();
                assert_eq!(
                    actual[normal],
                    page,
                    "step {} mode {mode} normal {normal}",
                    index / 2
                );
            }
        }
    }

    #[test]
    fn test_diffuse_shade_range() {
        let normal: Vec3 = Vec3::new(0.0, 0.0, 1.0);
        let light: Vec3 = Vec3::new(0.0, 0.0, 1.0);
        // Fully lit: ambient 0.6 + diffuse 0.4 * 1.0 = 1.0
        let bright: f32 = diffuse_shade(normal, light, 0.6, 0.4);
        assert!((bright - 1.0).abs() < 0.001);

        // Opposite direction: ambient only.
        let dark: f32 = diffuse_shade(normal, -light, 0.6, 0.4);
        assert!((dark - 0.6).abs() < 0.001);
    }
}
