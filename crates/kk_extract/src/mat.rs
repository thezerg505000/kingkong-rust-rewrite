//! Tiny 4x4 matrix helpers (row-major, row-vector convention as in the Jade data).

pub type M4 = [f64; 16];

pub const IDENT: M4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

/// `a @ b` (numpy convention).
pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for i in 0..4 {
        for j in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[i * 4 + k] * b[k * 4 + j];
            }
            o[i * 4 + j] = s;
        }
    }
    o
}

pub fn from_f32(a: &[f32; 16]) -> M4 {
    let mut o = [0.0; 16];
    for i in 0..16 {
        o[i] = a[i] as f64;
    }
    o
}

/// General inverse (Gauss-Jordan, partial pivoting).
pub fn inv(a: &M4) -> Option<M4> {
    let mut m = *a;
    let mut r = IDENT;
    for c in 0..4 {
        let mut p = c;
        for i in c + 1..4 {
            if m[i * 4 + c].abs() > m[p * 4 + c].abs() {
                p = i;
            }
        }
        if m[p * 4 + c].abs() < 1e-300 {
            return None;
        }
        if p != c {
            for j in 0..4 {
                m.swap(c * 4 + j, p * 4 + j);
                r.swap(c * 4 + j, p * 4 + j);
            }
        }
        let d = m[c * 4 + c];
        for j in 0..4 {
            m[c * 4 + j] /= d;
            r[c * 4 + j] /= d;
        }
        for i in 0..4 {
            if i != c {
                let f = m[i * 4 + c];
                if f != 0.0 {
                    for j in 0..4 {
                        m[i * 4 + j] -= f * m[c * 4 + j];
                        r[i * 4 + j] -= f * r[c * 4 + j];
                    }
                }
            }
        }
    }
    Some(r)
}

fn det3(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Rotation matrix (column-vector convention, rows r) -> quaternion xyzw (build_anim_glb.mat2quat).
pub fn mat2quat(r: &[[f64; 3]; 3]) -> [f64; 4] {
    let t = r[0][0] + r[1][1] + r[2][2];
    let mut q = [0.0f64; 4];
    if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        q = [(r[2][1] - r[1][2]) / s, (r[0][2] - r[2][0]) / s, (r[1][0] - r[0][1]) / s, s / 4.0];
    } else {
        let mut i = 0;
        for k in 1..3 {
            if r[k][k] > r[i][i] {
                i = k;
            }
        }
        let j = (i + 1) % 3;
        let k = (j + 1) % 3;
        let s = (r[i][i] - r[j][j] - r[k][k] + 1.0).sqrt() * 2.0;
        q[i] = s / 4.0;
        q[3] = (r[k][j] - r[j][k]) / s;
        q[j] = (r[j][i] + r[i][j]) / s;
        q[k] = (r[k][i] + r[i][k]) / s;
    }
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
}

/// Row-vector local matrix -> (translation, quaternion xyzw, scale). glTF rotation is `Rrow^T`.
pub fn decompose(l: &M4) -> Result<([f64; 3], [f64; 4], [f64; 3]), String> {
    let t = [l[12], l[13], l[14]];
    let mut s = [0.0; 3];
    let mut rn = [[0.0; 3]; 3];
    for i in 0..3 {
        s[i] = (l[i * 4] * l[i * 4] + l[i * 4 + 1] * l[i * 4 + 1] + l[i * 4 + 2] * l[i * 4 + 2]).sqrt();
        for j in 0..3 {
            rn[i][j] = l[i * 4 + j] / s[i];
        }
    }
    if det3(&rn) < 0.0 {
        return Err("mirrored bone".into());
    }
    let mut rt = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            rt[i][j] = rn[j][i];
        }
    }
    Ok((t, mat2quat(&rt), s))
}
