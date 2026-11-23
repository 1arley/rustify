pub(crate) const BLACK: u32 = 0x080808;
pub(crate) const SURFACE: u32 = 0x121212;
pub(crate) const CARD: u32 = 0x1b1b1b;
pub(crate) const CARD_HOVER: u32 = 0x292929;
pub(crate) const MUTED: u32 = 0xb3b3b3;
pub(crate) const PRIMARY: u32 = 0x282828;
pub(crate) const ACCENT: u32 = 0x1ed760;
pub(crate) const ERROR: u32 = 0xff4d4f;
pub(crate) const INFO: u32 = 0xb3b3b3;

const COVER_PALETTE: [u32; 8] = [
    0x1e90ff, 0x00c853, 0x8e24aa, 0xff6d00, 0xe91e63, 0x00acc1, 0x3949ab, 0xd81b60,
];

/// Escolhe uma cor vibrante determinística para placeholders de capa sem imagem.
pub(crate) fn cover_color(seed: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in seed.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    COVER_PALETTE[(hash as usize) % COVER_PALETTE.len()]
}
