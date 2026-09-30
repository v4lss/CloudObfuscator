use crate::names::NameFactory;
use crate::rng::Rng;
use std::collections::HashMap;

pub const DECODER_ARITY: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderKind {
    XorChain,
    Base64Custom,
    RotateArray,
    ReverseShift,
    SplitHalves,
}

impl EncoderKind {
    pub fn all() -> [EncoderKind; 5] {
        [
            EncoderKind::XorChain,
            EncoderKind::Base64Custom,
            EncoderKind::RotateArray,
            EncoderKind::ReverseShift,
            EncoderKind::SplitHalves,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            EncoderKind::XorChain => "xor-chain",
            EncoderKind::Base64Custom => "base64-custom",
            EncoderKind::RotateArray => "rotate-array",
            EncoderKind::ReverseShift => "reverse-shift",
            EncoderKind::SplitHalves => "split-halves",
        }
    }
}

const STANDARD_B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn escape_js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn units(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn byte_array(values: &[u8]) -> String {
    let mut parts = Vec::with_capacity(values.len());
    for value in values {
        parts.push(value.to_string());
    }
    format!("[{}]", parts.join(","))
}

fn xor_chain_encode(value: &str, key: u16) -> Vec<u16> {
    let mut acc = key as u32;
    units(value)
        .iter()
        .map(|unit| {
            acc ^= *unit as u32;
            (acc & 0xFFFF) as u16
        })
        .collect()
}

fn base64_indices(value: &str) -> Vec<u8> {
    let bytes: Vec<u8> = units(value)
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(((triple >> 18) & 63) as u8);
        out.push(((triple >> 12) & 63) as u8);
        out.push(if chunk.len() > 1 {
            ((triple >> 6) & 63) as u8
        } else {
            64
        });
        out.push(if chunk.len() > 2 {
            (triple & 63) as u8
        } else {
            64
        });
    }
    out
}

fn reverse_shift_encode(value: &str, key: u16) -> Vec<u16> {
    units(value)
        .iter()
        .enumerate()
        .map(|(index, unit)| ((*unit as u32 + key as u32 + index as u32) & 0xFFFF) as u16)
        .collect()
}

#[derive(Debug, Clone)]
struct StringGroup {
    kind: EncoderKind,
    decoder: String,
    source: String,
}

#[derive(Debug, Clone)]
pub struct StringTable {
    groups: Vec<StringGroup>,
    index_of: HashMap<String, (usize, usize)>,
    prelude: String,
}

impl StringTable {
    pub fn build(
        rng: &mut Rng,
        names: &mut NameFactory,
        values: &[String],
        group_count: usize,
    ) -> StringTable {
        let kinds = EncoderKind::all();
        let cursor = rng.below(kinds.len());
        let groups = group_count.clamp(1, 8).max(1);
        let plan: Vec<EncoderKind> = (0..groups)
            .map(|index| kinds[(cursor + index) % kinds.len()])
            .collect();
        StringTable::build_with_kinds(rng, names, values, &plan)
    }

    pub fn build_with_kinds(
        rng: &mut Rng,
        names: &mut NameFactory,
        values: &[String],
        plan: &[EncoderKind],
    ) -> StringTable {
        let mut index_of: HashMap<String, (usize, usize)> = HashMap::new();
        let mut groups: Vec<StringGroup> = Vec::new();
        let mut prelude = String::new();

        if values.is_empty() || plan.is_empty() {
            return StringTable {
                groups,
                index_of,
                prelude,
            };
        }

        let group_count = plan.len().min(values.len()).max(1);
        let per_group = values.len().div_ceil(group_count);

        for (group_index, chunk) in values.chunks(per_group).enumerate() {
            if chunk.is_empty() {
                continue;
            }

            let kind = plan[group_index % plan.len()];
            let decoder = names.next(rng);
            let table = names.next(rng);
            let key = names.next(rng);
            let param = names.next(rng);
            let out = names.next(rng);
            let cursor = names.next(rng);
            let alphabet = names.next(rng);
            let rotate_fn = names.next(rng);
            let scratch = names.next(rng);

            let offset = if rng.chance(45) {
                rng.i32_range(1, 5)
            } else {
                0
            };
            let length = chunk.len();
            let shift = offset.rem_euclid(length as i32) as usize;
            let ordered: Vec<usize> = (0..length)
                .map(|slot| (slot + length - shift) % length)
                .collect();

            let source = match kind {
                EncoderKind::XorChain => {
                    let keys: Vec<u16> = (0..length).map(|_| rng.range(1, 0x7FFF) as u16).collect();
                    let payload = ordered
                        .iter()
                        .zip(keys.iter())
                        .map(|(local, k)| xor_chain_encode(&chunk[*local], *k))
                        .map(|encoded| {
                            let parts: Vec<String> =
                                encoded.iter().map(|v| v.to_string()).collect();
                            format!("[{}]", parts.join(","))
                        })
                        .collect::<Vec<String>>()
                        .join(",");
                    let keys_src = keys
                        .iter()
                        .map(|k| k.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(
                        "var {table}=[{payload}],{key}=[{keys_src}];\
                         var {decoder}=function({param}){{var a={table}[{param}],\
                         {scratch}={key}[{param}],{out}=\"\",{cursor}=0,x={scratch};\
                         for({cursor}=0;{cursor}<a.length;{cursor}++)\
                         {{{out}+=String.fromCharCode(x^a[{cursor}]);x=a[{cursor}];}}\
                         return {out};}};",
                        table = table,
                        payload = payload,
                        key = key,
                        keys_src = keys_src,
                        decoder = decoder,
                        param = param,
                        scratch = scratch,
                        out = out,
                        cursor = cursor
                    )
                }
                EncoderKind::Base64Custom => {
                    let mut chars: Vec<char> = STANDARD_B64.iter().map(|b| *b as char).collect();
                    for index in (1..chars.len()).rev() {
                        let swap = rng.below(index + 1);
                        chars.swap(index, swap);
                    }
                    let custom: String = chars.iter().collect();
                    let mut position = [0u8; 64];
                    for (slot, c) in chars.iter().enumerate() {
                        let standard = STANDARD_B64
                            .iter()
                            .position(|b| *b == *c as u8)
                            .unwrap_or_default();
                        position[standard] = slot as u8;
                    }
                    let payload = ordered
                        .iter()
                        .map(|local| {
                            let value = &chunk[*local];
                            let mut row: Vec<u8> = vec![units(value).len() as u8];
                            row.extend(base64_indices(value).into_iter().map(|i| {
                                if i >= 64 {
                                    0
                                } else {
                                    position[i as usize]
                                }
                            }));
                            while row.len() % 4 != 1 {
                                row.push(0);
                            }
                            byte_array(&row)
                        })
                        .collect::<Vec<String>>()
                        .join(",");
                    let inverse: Vec<String> = chars
                        .iter()
                        .map(|c| {
                            STANDARD_B64
                                .iter()
                                .position(|b| *b == *c as u8)
                                .unwrap_or(0)
                                .to_string()
                        })
                        .collect();
                    format!(
                        "var {alphabet}={custom},{scratch}=[{inverse}];\
                         var {table}=[{payload}];\
                         var {decoder}=function({param}){{var a={table}[{param}],n=a[0],\
                         {out}=\"\",{cursor}=1,v=0,c=\"\";for(;{cursor}+3<a.length;{cursor}+=4)\
                         {{v={scratch}[a[{cursor}]]<<18|{scratch}[a[{cursor}+1]]<<12\
                         |{scratch}[a[{cursor}+2]]<<6|{scratch}[a[{cursor}+3]];\
                         c+=String.fromCharCode(v>>16&255,v>>8&255,v&255);}}\
                         for({cursor}=0;{cursor}<n*2;{cursor}+=2)\
                         {{{out}+=String.fromCharCode(c.charCodeAt({cursor})\
                         |c.charCodeAt({cursor}+1)<<8);}}return {out};}};",
                        alphabet = alphabet,
                        custom = escape_js_string(&custom),
                        scratch = scratch,
                        inverse = inverse.join(","),
                        table = table,
                        payload = payload,
                        decoder = decoder,
                        param = param,
                        out = out,
                        cursor = cursor
                    )
                }
                EncoderKind::RotateArray => {
                    let rotation = rng.below(length);
                    for (position, local) in ordered.iter().enumerate() {
                        index_of.insert(
                            chunk[*local].clone(),
                            (group_index, (position + length - rotation) % length),
                        );
                    }
                    let keys: Vec<u16> = (0..length).map(|_| rng.range(1, 0x7FFF) as u16).collect();
                    let payload = ordered
                        .iter()
                        .zip(keys.iter())
                        .map(|(local, key)| reverse_shift_encode(&chunk[*local], *key))
                        .map(|encoded| {
                            let parts: Vec<String> =
                                encoded.iter().map(|value| value.to_string()).collect();
                            format!("[{}]", parts.join(","))
                        })
                        .collect::<Vec<String>>()
                        .join(",");
                    let keys_src = keys
                        .iter()
                        .map(|key| key.to_string())
                        .collect::<Vec<String>>()
                        .join(",");
                    let rotate = format!(
                        "(function({table},{key},{rotate_fn}){{try{{for(;{rotate_fn}>0;{rotate_fn}--){{\
                         {table}.push({table}.shift());{key}.push({key}.shift());}}}}\
                         catch({out}){{}}}})({table},{key},{rotation});",
                        table = table,
                        key = key,
                        rotate_fn = rotate_fn,
                        rotation = rotation,
                        out = out
                    );
                    prelude.push_str(&rotate);
                    prelude.push('\n');
                    format!(
                        "var {table}=[{payload}],{key}=[{keys_src}];\
                         var {decoder}=function({param}){{var a={table}[{param}],\
                         b={key}[{param}],{out}=\"\",{cursor}=a.length-1;\
                         for(;{cursor}>=0;{cursor}--)\
                         {{{out}+=String.fromCharCode(a[{cursor}]-(b+{cursor}&65535));}}\
                         return {out}.split(\"\").reverse().join(\"\");}};",
                        table = table,
                        payload = payload,
                        key = key,
                        keys_src = keys_src,
                        decoder = decoder,
                        param = param,
                        out = out,
                        cursor = cursor
                    )
                }
                EncoderKind::ReverseShift => {
                    let keys: Vec<u16> = (0..length).map(|_| rng.range(1, 0x7FFF) as u16).collect();
                    let payload = ordered
                        .iter()
                        .zip(keys.iter())
                        .map(|(local, k)| reverse_shift_encode(&chunk[*local], *k))
                        .map(|encoded| {
                            let parts: Vec<String> =
                                encoded.iter().map(|v| v.to_string()).collect();
                            format!("[{}]", parts.join(","))
                        })
                        .collect::<Vec<String>>()
                        .join(",");
                    let keys_src = keys
                        .iter()
                        .map(|k| k.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(
                        "var {table}=[{payload}],{key}=[{keys_src}];\
                         var {decoder}=function({param}){{var a={table}[{param}],\
                         b={key}[{param}],{out}=\"\",{cursor}=a.length-1;\
                         for(;{cursor}>=0;{cursor}--)\
                         {{{out}+=String.fromCharCode(a[{cursor}]-(b+{cursor}&65535));}}\
                         return {out}.split(\"\").reverse().join(\"\");}};",
                        table = table,
                        payload = payload,
                        key = key,
                        keys_src = keys_src,
                        decoder = decoder,
                        param = param,
                        out = out,
                        cursor = cursor
                    )
                }
                EncoderKind::SplitHalves => {
                    let mut left_src_parts = Vec::new();
                    let mut right_src_parts = Vec::new();
                    for local in &ordered {
                        let units = units(&chunk[*local]);
                        let split = units.len() / 2 + units.len() % 2;
                        let head: Vec<String> =
                            units[..split].iter().map(|u| u.to_string()).collect();
                        let tail: Vec<String> =
                            units[split..].iter().map(|u| u.to_string()).collect();
                        left_src_parts.push(format!("[{}]", head.join(",")));
                        right_src_parts.push(format!("[{}]", tail.join(",")));
                    }
                    let base = rng.range(1024, 8192);
                    format!(
                        "var {table}=[].concat(new Array({stride}),[{left_src}]),\
                         {key}=[].concat(new Array({stride}),[{right_src}]);\
                         var {decoder}=function({param}){{var v={param}+{stride},\
                         l={table}[v],r={key}[v],{out}=\"\",{cursor}=0;\
                         for(;{cursor}<l.length;{cursor}++)\
                         {{{out}+=String.fromCharCode(l[{cursor}]);}}\
                         for({cursor}=0;{cursor}<r.length;{cursor}++)\
                         {{{out}+=String.fromCharCode(r[{cursor}]);}}return {out};}};",
                        table = table,
                        stride = base,
                        left_src = left_src_parts.join(","),
                        right_src = right_src_parts.join(","),
                        key = key,
                        decoder = decoder,
                        param = param,
                        out = out,
                        cursor = cursor
                    )
                }
            };

            if kind != EncoderKind::RotateArray {
                for (slot, local) in ordered.iter().enumerate() {
                    index_of.insert(chunk[*local].clone(), (group_index, slot));
                }
            }

            groups.push(StringGroup {
                kind,
                decoder,
                source,
            });
        }

        StringTable {
            groups,
            index_of,
            prelude,
        }
    }

    pub fn len(&self) -> usize {
        self.index_of.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index_of.is_empty()
    }

    pub fn decoder_count(&self) -> usize {
        self.groups.len()
    }

    pub fn kinds(&self) -> Vec<EncoderKind> {
        self.groups.iter().map(|g| g.kind).collect()
    }

    pub fn decoder_names(&self) -> Vec<String> {
        self.groups.iter().map(|g| g.decoder.clone()).collect()
    }

    pub fn decoder_arities(&self) -> Vec<usize> {
        self.groups.iter().map(|_| DECODER_ARITY).collect()
    }

    pub fn kind_labels(&self) -> Vec<&'static str> {
        self.groups.iter().map(|g| g.kind.label()).collect()
    }

    pub fn accessor(&self, value: &str) -> Option<String> {
        let (group_index, slot) = *self.index_of.get(value)?;
        let group = self.groups.get(group_index)?;
        Some(format!("{}({})", group.decoder, slot))
    }

    pub fn source(&self) -> String {
        let mut out = String::new();
        for group in &self.groups {
            out.push_str(&group.source);
            out.push('\n');
        }
        out.push_str(&self.prelude);
        out
    }
}
