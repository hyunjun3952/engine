use font8x8::UnicodeFonts;

pub(crate) const GLYPHS: usize = 128;

pub(crate) fn bits(code: u8) -> [u8; 8] {
    font8x8::BASIC_FONTS.get(code as char).unwrap_or([0; 8])
}
