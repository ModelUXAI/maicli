//! Embedding lokal deterministik: hashed n-gram, tanpa model/jaringan.
//!
//! Kualitas semantiknya di bawah model neural, tetapi cukup untuk pencarian
//! memori proyek dan tidak menambah dependensi ML besar.

/// Dimensi vektor embedding.
pub const DIM: usize = 384;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Vektor embedding untuk satu teks.
pub fn embed(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0f32; DIM];
    let lower = text.to_lowercase();
    for token in lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let bytes = token.as_bytes();
        let hash = fnv1a(bytes);
        let index = (hash as usize) % DIM;
        let sign = if hash & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
        // Trigrams karakter menangkap kemiripan bentuk kata (mis. "memori"/"memory").
        let chars: Vec<char> = token.chars().collect();
        for window in chars.windows(3) {
            let gram: String = window.iter().collect();
            let hash = fnv1a(gram.as_bytes());
            let index = (hash as usize) % DIM;
            let sign = if hash & 1 == 0 { 0.5 } else { -0.5 };
            vector[index] += sign;
        }
    }
    normalize(&mut vector);
    vector
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

/// Cosine similarity; vektor nol menghasilkan 0.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for index in 0..len {
        dot += a[index] * b[index];
        na += a[index] * a[index];
        nb += b[index] * b[index];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vektor_ternormalisasi_dan_stabil() {
        let a = embed("gateway memakai rustls");
        let b = embed("gateway memakai rustls");
        assert_eq!(a, b);
        let norm: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "norm {norm}");
    }

    #[test]
    fn teks_mirip_lebih_dekat_dari_teks_lain() {
        let query = embed("rustls gateway");
        let dekat = embed("gateway memakai rustls tanpa openssl");
        let jauh = embed("resep nasi goreng telur");
        assert!(cosine(&query, &dekat) > cosine(&query, &jauh));
    }

    #[test]
    fn teks_kosong_tidak_panic() {
        let vector = embed("");
        assert_eq!(vector.len(), DIM);
        assert!(vector.iter().all(|v| *v == 0.0));
    }
}
