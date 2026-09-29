use rand::Rng as _;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn derive_seed(explicit: Option<u64>) -> u64 {
    if let Some(seed) = explicit {
        return seed;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hash = nanos ^ (counter.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    hash ^= hash.rotate_left(31);
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash.rotate_right(27);
    hash.wrapping_mul(0x94d0_49bb_1331_11eb)
}

pub struct Rng {
    inner: ChaCha8Rng,
    pub seed: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut bytes = [0u8; 32];
        let mut state = seed;
        for chunk in bytes.chunks_mut(8) {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes());
        }
        Rng {
            inner: ChaCha8Rng::from_seed(bytes),
            seed,
        }
    }

    pub fn u32(&mut self) -> u32 {
        self.inner.random()
    }

    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.u64() % bound as u64) as usize
    }

    pub fn u64(&mut self) -> u64 {
        let hi = self.u32() as u64;
        let lo = self.u32() as u64;
        (hi << 32) | lo
    }

    pub fn range(&mut self, low: usize, high: usize) -> usize {
        if high <= low {
            return low;
        }
        low + self.below(high - low)
    }

    pub fn i32_range(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        low + (self.below((high - low) as usize) as i32)
    }

    pub fn chance(&mut self, percent: u8) -> bool {
        self.below(100) < percent as usize
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let index = self.below(items.len());
            items.get(index)
        }
    }

    pub fn sample_string(&mut self, alphabet: &[char], length: usize) -> String {
        let mut out = String::with_capacity(length);
        for _ in 0..length {
            let index = self.below(alphabet.len());
            if let Some(c) = alphabet.get(index) {
                out.push(*c);
            }
        }
        out
    }

    pub fn bytes(&mut self, length: usize) -> Vec<u8> {
        let mut out = vec![0u8; length];
        for slot in out.iter_mut() {
            *slot = self.u32() as u8;
        }
        out
    }
}
