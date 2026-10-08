//! Minimal glTF 2.0 `.glb` writer (port of `tools/glbwriter.py` + the write paths of
//! `build_anim_glb.py` / `texture_bind.py`): meshes with several primitives, skin, animations,
//! embedded PNG images.

use serde_json::{json, Map, Value};

pub const F32: u32 = 5126;
pub const U16: u32 = 5123;
pub const U32: u32 = 5125;

/// f32 -> JSON number via its shortest decimal form (keeps the files small).
pub fn jf(x: f32) -> Value {
    if !x.is_finite() {
        return Value::Null;
    }
    let s = format!("{x}");
    serde_json::Number::from_f64(s.parse::<f64>().unwrap_or(x as f64)).map(Value::Number).unwrap_or(Value::Null)
}
pub fn jd(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}
pub fn jf_arr(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|&x| jf(x)).collect())
}

#[derive(Default)]
pub struct Glb {
    pub bin: Vec<u8>,
    pub buffer_views: Vec<Value>,
    pub accessors: Vec<Value>,
    pub nodes: Vec<Value>,
    pub meshes: Vec<Value>,
    pub skins: Vec<Value>,
    pub materials: Vec<Value>,
    pub animations: Vec<Value>,
    pub images: Vec<Value>,
    pub textures: Vec<Value>,
    pub samplers: Vec<Value>,
    pub scene_roots: Vec<usize>,
}

impl Glb {
    pub fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        let off = self.bin.len();
        self.bin.extend_from_slice(data);
        let mut v = json!({"buffer": 0, "byteOffset": off, "byteLength": data.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.buffer_views.push(v);
        self.buffer_views.len() - 1
    }

    fn push_acc(&mut self, bv: usize, ctype: u32, count: usize, typ: &str, min: Option<Vec<f32>>, max: Option<Vec<f32>>) -> usize {
        let mut a = json!({"bufferView": bv, "componentType": ctype, "count": count, "type": typ});
        if let (Some(mn), Some(mx)) = (min, max) {
            a["min"] = jf_arr(&mn);
            a["max"] = jf_arr(&mx);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    /// f32 accessor with `comps` components per element.
    pub fn acc_f32(&mut self, data: &[f32], comps: usize, typ: &str, target: Option<u32>, minmax: bool) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for v in data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let bv = self.view(&bytes, target);
        let (mn, mx) = if minmax && !data.is_empty() {
            let mut mn = vec![f32::INFINITY; comps];
            let mut mx = vec![f32::NEG_INFINITY; comps];
            for e in data.chunks_exact(comps) {
                for c in 0..comps {
                    mn[c] = mn[c].min(e[c]);
                    mx[c] = mx[c].max(e[c]);
                }
            }
            (Some(mn), Some(mx))
        } else {
            (None, None)
        };
        self.push_acc(bv, F32, data.len() / comps, typ, mn, mx)
    }
    pub fn acc_u16(&mut self, data: &[u16], comps: usize, typ: &str, target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 2);
        for v in data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let bv = self.view(&bytes, target);
        self.push_acc(bv, U16, data.len() / comps, typ, None, None)
    }
    pub fn acc_u32(&mut self, data: &[u32], target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for v in data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let bv = self.view(&bytes, target);
        self.push_acc(bv, U32, data.len(), "SCALAR", None, None)
    }

    /// Embed a PNG; returns the texture index (image + texture sharing sampler 0).
    pub fn add_png_texture(&mut self, name: &str, png: &[u8]) -> usize {
        let bv = self.view(png, None);
        self.images.push(json!({"bufferView": bv, "mimeType": "image/png", "name": name}));
        self.textures.push(json!({"source": self.images.len() - 1, "sampler": 0}));
        self.textures.len() - 1
    }

    pub fn finish(self, asset_extras: Value, generator: &str) -> Vec<u8> {
        let mut doc = Map::new();
        let mut asset = json!({"version": "2.0", "generator": generator});
        if !asset_extras.is_null() {
            asset["extras"] = asset_extras;
        }
        doc.insert("asset".into(), asset);
        doc.insert("scene".into(), json!(0));
        doc.insert("scenes".into(), json!([{"nodes": self.scene_roots}]));
        doc.insert("nodes".into(), Value::Array(self.nodes));
        doc.insert("meshes".into(), Value::Array(self.meshes));
        doc.insert("accessors".into(), Value::Array(self.accessors));
        doc.insert("bufferViews".into(), Value::Array(self.buffer_views));
        let mut bin = self.bin;
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        doc.insert("buffers".into(), json!([{"byteLength": bin.len()}]));
        for (k, v) in [("skins", self.skins), ("materials", self.materials), ("animations", self.animations), ("images", self.images), ("textures", self.textures), ("samplers", self.samplers)] {
            if !v.is_empty() {
                doc.insert(k.into(), Value::Array(v));
            }
        }
        let mut js = serde_json::to_vec(&Value::Object(doc)).expect("json");
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

/// Parse a .glb into (json, bin) -- used by tests and by readers of existing assets.
pub fn read_glb(b: &[u8]) -> Result<(Value, Vec<u8>), String> {
    if b.len() < 20 || &b[0..4] != b"glTF" {
        return Err("not a glb".into());
    }
    let jl = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
    let js: Value = serde_json::from_slice(b.get(20..20 + jl).ok_or("short json")?).map_err(|e| e.to_string())?;
    let off = 20 + jl;
    let bl = u32::from_le_bytes(b.get(off..off + 4).ok_or("short bin")?.try_into().unwrap()) as usize;
    let bin = b.get(off + 8..off + 8 + bl).ok_or("short bin chunk")?.to_vec();
    Ok((js, bin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let mut g = Glb::default();
        let a = g.acc_f32(&[0.0, 1.0, 2.0, -1.0, 5.0, 3.0], 3, "VEC3", Some(34962), true);
        let i = g.acc_u32(&[0, 1, 2], Some(34963));
        let t = g.add_png_texture("t", &[1, 2, 3]);
        g.samplers.push(json!({"magFilter": 9729}));
        g.nodes.push(json!({"name": "n"}));
        g.scene_roots.push(0);
        assert_eq!((a, i, t), (0, 1, 0));
        let out = g.finish(json!({"k": 1}), "test");
        assert_eq!(out.len() % 4, 0);
        let (js, bin) = read_glb(&out).unwrap();
        assert_eq!(js["accessors"][0]["max"], json!([0.0, 5.0, 3.0]));
        assert_eq!(js["accessors"][0]["min"], json!([-1.0, 1.0, 2.0]));
        assert_eq!(js["asset"]["extras"]["k"], 1);
        assert_eq!(js["buffers"][0]["byteLength"].as_u64().unwrap() as usize, bin.len());
        let bv = &js["bufferViews"][js["images"][0]["bufferView"].as_u64().unwrap() as usize];
        let o = bv["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(&bin[o..o + 3], &[1, 2, 3]);
    }

    #[test]
    fn jf_is_short() {
        assert_eq!(jf(0.1).to_string(), "0.1");
        assert_eq!(jf(f32::NAN), Value::Null);
    }
}
