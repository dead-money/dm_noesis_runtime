//! Mirrors of the public Noesis types in `Include/NsRender/RenderDevice.h`.
//!
//! These types cross the FFI boundary into our C++ shim and on into Noesis,
//! so any drift from the Noesis-side declarations is a hard ABI bug. Layout
//! is verified at compile time at the bottom of this file.
//!
//! ABI notes:
//! - Unscoped C++ `enum`s default to `int` (4 bytes on Linux x86-64).
//!   `#[repr(C)]` Rust enums match that.
//! - `Shader`, `SamplerState`, and `RenderState` are stored as a single
//!   `uint8_t` in `Batch`. We mirror them as `#[repr(transparent)]` newtypes
//!   over `u8` rather than Rust enums — that preserves the size *and* keeps
//!   any incoming byte value valid (no UB if Noesis adds variants we haven't
//!   mirrored yet).
//! - Bitfield ordering follows the LSB-first convention used by GCC and
//!   Clang on x86-64 / aarch64 / wasm targets.

#![allow(clippy::enum_variant_names)] // mirroring Noesis-side names verbatim

use core::mem::{align_of, size_of};
use std::os::raw::c_void;

// ────────────────────────────────────────────────────────────────────────────
// Texture formats — `Noesis::TextureFormat::Enum`
// ────────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    /// Four-component, 8 bits per channel including alpha.
    Rgba8 = 0,
    /// Four-component, 8 bits per color channel + 8 bits unused.
    Rgbx8 = 1,
    /// Single-component, 8 bits red.
    R8 = 2,
}

pub const TEXTURE_FORMAT_COUNT: usize = 3;

// ────────────────────────────────────────────────────────────────────────────
// Sampler state — `Noesis::WrapMode::Enum`, `MinMagFilter::Enum`,
// `MipFilter::Enum`, `Noesis::SamplerState`
// ────────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WrapMode {
    /// Clamp UV between 0.0 and 1.0.
    ClampToEdge = 0,
    /// Out-of-range coordinates return transparent zero.
    ClampToZero = 1,
    Repeat = 2,
    /// Repeat with horizontal flip.
    MirrorU = 3,
    /// Repeat with vertical flip.
    MirrorV = 4,
    /// Combination of `MirrorU` and `MirrorV`.
    Mirror = 5,
}

pub const WRAP_MODE_COUNT: usize = 6;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MinMagFilter {
    Nearest = 0,
    Linear = 1,
}

pub const MIN_MAG_FILTER_COUNT: usize = 2;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MipFilter {
    /// Sample from mipmap level 0 only.
    Disabled = 0,
    Nearest = 1,
    Linear = 2,
}

pub const MIP_FILTER_COUNT: usize = 3;

/// Mirror of `Noesis::SamplerState`.
///
/// Packed bitfield in a single byte: bits 0–2 wrap mode, bit 3 min/mag
/// filter, bits 4–5 mip filter, bits 6–7 unused.
#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct SamplerState(pub u8);

impl SamplerState {
    /// Pack the three sampler fields into the canonical byte layout.
    #[must_use]
    pub const fn new(wrap: WrapMode, minmag: MinMagFilter, mip: MipFilter) -> Self {
        let bits = (wrap as u8 & 0b111)
            | ((minmag as u8 & 0b1) << 3)
            | ((mip as u8 & 0b11) << 4);
        Self(bits)
    }

    /// Raw 3-bit wrap-mode field. Matches `WrapMode as u8` for valid values.
    #[must_use]
    pub const fn wrap_mode_raw(self) -> u8 {
        self.0 & 0b111
    }

    /// Raw 1-bit min/mag filter field. Matches `MinMagFilter as u8`.
    #[must_use]
    pub const fn minmag_filter_raw(self) -> u8 {
        (self.0 >> 3) & 0b1
    }

    /// Raw 2-bit mip filter field. Matches `MipFilter as u8`.
    #[must_use]
    pub const fn mip_filter_raw(self) -> u8 {
        (self.0 >> 4) & 0b11
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Blend & stencil — `Noesis::BlendMode::Enum`, `Noesis::StencilMode::Enum`,
// `Noesis::RenderState`
// ────────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// `cs / as`
    Src = 0,
    /// `cs + cd*(1-as) / as + ad*(1-as)` — standard premultiplied alpha.
    SrcOver = 1,
    /// `cs * cd + cd*(1-as) / as + ad*(1-as)`.
    SrcOverMultiply = 2,
    /// `cs + cd*(1-cs) / as + ad*(1-as)`.
    SrcOverScreen = 3,
    /// Additive: `cs + cs / as + ad*(1-as)`.
    SrcOverAdditive = 4,
    /// Dual-source blending; needed for SDF subpixel rendering.
    SrcOverDual = 5,
}

pub const BLEND_MODE_COUNT: usize = 6;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StencilMode {
    Disabled = 0,
    EqualKeep = 1,
    EqualIncr = 2,
    EqualDecr = 3,
    /// Set stencil data to 0.
    Clear = 4,
    /// Stencil disabled, depth test enabled.
    DisabledZTest = 5,
    /// Stencil and depth test both enabled.
    EqualKeepZTest = 6,
}

pub const STENCIL_MODE_COUNT: usize = 7;

/// Mirror of `Noesis::RenderState`.
///
/// Packed bitfield in a single byte: bit 0 colorEnable, bits 1–3 blendMode,
/// bits 4–6 stencilMode, bit 7 wireframe.
#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct RenderState(pub u8);

impl RenderState {
    #[must_use]
    pub const fn new(
        color_enable: bool,
        blend: BlendMode,
        stencil: StencilMode,
        wireframe: bool,
    ) -> Self {
        let bits = (color_enable as u8 & 0b1)
            | ((blend as u8 & 0b111) << 1)
            | ((stencil as u8 & 0b111) << 4)
            | ((wireframe as u8 & 0b1) << 7);
        Self(bits)
    }

    #[must_use]
    pub const fn color_enable(self) -> bool {
        (self.0 & 0b1) != 0
    }

    #[must_use]
    pub const fn blend_mode_raw(self) -> u8 {
        (self.0 >> 1) & 0b111
    }

    #[must_use]
    pub const fn stencil_mode_raw(self) -> u8 {
        (self.0 >> 4) & 0b111
    }

    #[must_use]
    pub const fn wireframe(self) -> bool {
        (self.0 >> 7) != 0
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Shader / vertex / format taxonomy — `Noesis::Shader` and nested types
// ────────────────────────────────────────────────────────────────────────────

/// Mirror of `Noesis::Shader`.
///
/// The C++ side is a struct with a single `uint8_t v` field. We use a
/// transparent newtype rather than a Rust enum so any incoming byte stays
/// valid — Noesis is allowed to extend the variant set in a point release
/// without us reading uninitialised discriminants.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shader(pub u8);

#[allow(non_upper_case_globals)] // intentionally not — see consts below
impl Shader {
    // ─── Debug ────────────────────────────────────────────────────────────
    pub const RGBA: Self = Self(0);
    /// Stencil-only rendering for masks.
    pub const MASK: Self = Self(1);
    /// Clear render target.
    pub const CLEAR: Self = Self(2);

    // ─── Path (no PPAA) ───────────────────────────────────────────────────
    pub const PATH_SOLID: Self = Self(3);
    pub const PATH_LINEAR: Self = Self(4);
    pub const PATH_RADIAL: Self = Self(5);
    pub const PATH_PATTERN: Self = Self(6);
    pub const PATH_PATTERN_CLAMP: Self = Self(7);
    pub const PATH_PATTERN_REPEAT: Self = Self(8);
    pub const PATH_PATTERN_MIRROR_U: Self = Self(9);
    pub const PATH_PATTERN_MIRROR_V: Self = Self(10);
    pub const PATH_PATTERN_MIRROR: Self = Self(11);

    // ─── Path (with PPAA) ─────────────────────────────────────────────────
    pub const PATH_AA_SOLID: Self = Self(12);
    pub const PATH_AA_LINEAR: Self = Self(13);
    pub const PATH_AA_RADIAL: Self = Self(14);
    pub const PATH_AA_PATTERN: Self = Self(15);
    pub const PATH_AA_PATTERN_CLAMP: Self = Self(16);
    pub const PATH_AA_PATTERN_REPEAT: Self = Self(17);
    pub const PATH_AA_PATTERN_MIRROR_U: Self = Self(18);
    pub const PATH_AA_PATTERN_MIRROR_V: Self = Self(19);
    pub const PATH_AA_PATTERN_MIRROR: Self = Self(20);

    // ─── SDF (text) ───────────────────────────────────────────────────────
    pub const SDF_SOLID: Self = Self(21);
    pub const SDF_LINEAR: Self = Self(22);
    pub const SDF_RADIAL: Self = Self(23);
    pub const SDF_PATTERN: Self = Self(24);
    pub const SDF_PATTERN_CLAMP: Self = Self(25);
    pub const SDF_PATTERN_REPEAT: Self = Self(26);
    pub const SDF_PATTERN_MIRROR_U: Self = Self(27);
    pub const SDF_PATTERN_MIRROR_V: Self = Self(28);
    pub const SDF_PATTERN_MIRROR: Self = Self(29);

    // ─── SDF LCD (subpixel text; needs DeviceCaps::subpixelRendering) ─────
    pub const SDF_LCD_SOLID: Self = Self(30);
    pub const SDF_LCD_LINEAR: Self = Self(31);
    pub const SDF_LCD_RADIAL: Self = Self(32);
    pub const SDF_LCD_PATTERN: Self = Self(33);
    pub const SDF_LCD_PATTERN_CLAMP: Self = Self(34);
    pub const SDF_LCD_PATTERN_REPEAT: Self = Self(35);
    pub const SDF_LCD_PATTERN_MIRROR_U: Self = Self(36);
    pub const SDF_LCD_PATTERN_MIRROR_V: Self = Self(37);
    pub const SDF_LCD_PATTERN_MIRROR: Self = Self(38);

    // ─── Opacity (offscreen) ──────────────────────────────────────────────
    pub const OPACITY_SOLID: Self = Self(39);
    pub const OPACITY_LINEAR: Self = Self(40);
    pub const OPACITY_RADIAL: Self = Self(41);
    pub const OPACITY_PATTERN: Self = Self(42);
    pub const OPACITY_PATTERN_CLAMP: Self = Self(43);
    pub const OPACITY_PATTERN_REPEAT: Self = Self(44);
    pub const OPACITY_PATTERN_MIRROR_U: Self = Self(45);
    pub const OPACITY_PATTERN_MIRROR_V: Self = Self(46);
    pub const OPACITY_PATTERN_MIRROR: Self = Self(47);

    // ─── Misc ─────────────────────────────────────────────────────────────
    pub const UPSAMPLE: Self = Self(48);
    pub const DOWNSAMPLE: Self = Self(49);
    pub const SHADOW: Self = Self(50);
    pub const BLUR: Self = Self(51);
    pub const CUSTOM_EFFECT: Self = Self(52);
}

pub const SHADER_COUNT: usize = 53;

/// Mirror of `Noesis::Shader::Vertex::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VertexShader {
    Pos = 0,
    PosColor = 1,
    PosTex0 = 2,
    PosTex0Rect = 3,
    PosTex0RectTile = 4,
    PosColorCoverage = 5,
    PosTex0Coverage = 6,
    PosTex0CoverageRect = 7,
    PosTex0CoverageRectTile = 8,
    PosColorTex1Sdf = 9,
    PosTex0Tex1Sdf = 10,
    PosTex0Tex1RectSdf = 11,
    PosTex0Tex1RectTileSdf = 12,
    PosColorTex1 = 13,
    PosTex0Tex1 = 14,
    PosTex0Tex1Rect = 15,
    PosTex0Tex1RectTile = 16,
    PosColorTex0Tex1 = 17,
    PosTex0Tex1Downsample = 18,
    PosColorTex1Rect = 19,
    PosColorTex0RectImagePos = 20,
}

pub const VERTEX_SHADER_COUNT: usize = 21;

/// Mirror of `Noesis::Shader::Vertex::Format::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    Pos = 0,
    PosColor = 1,
    PosTex0 = 2,
    PosTex0Rect = 3,
    PosTex0RectTile = 4,
    PosColorCoverage = 5,
    PosTex0Coverage = 6,
    PosTex0CoverageRect = 7,
    PosTex0CoverageRectTile = 8,
    PosColorTex1 = 9,
    PosTex0Tex1 = 10,
    PosTex0Tex1Rect = 11,
    PosTex0Tex1RectTile = 12,
    PosColorTex0Tex1 = 13,
    PosColorTex1Rect = 14,
    PosColorTex0RectImagePos = 15,
}

pub const VERTEX_FORMAT_COUNT: usize = 16;

/// Mirror of `Noesis::Shader::Vertex::Format::Attr::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VertexAttr {
    /// Position (xy), linear.
    Pos = 0,
    /// sRGB color (rgba), nointerpolation.
    Color = 1,
    /// `TexCoord0` (uv), linear.
    Tex0 = 2,
    /// `TexCoord1` (uv), linear.
    Tex1 = 3,
    /// Coverage (alpha), linear.
    Coverage = 4,
    /// Rect (x0, y0, x1, y1), nointerpolation.
    Rect = 5,
    /// Tile rect (x, y, w, h), nointerpolation.
    Tile = 6,
    /// Position (xy) + scale (zw), linear.
    ImagePos = 7,
}

pub const VERTEX_ATTR_COUNT: usize = 8;

/// Mirror of `Noesis::Shader::Vertex::Format::Attr::Type::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VertexAttrType {
    /// One 32-bit float.
    Float = 0,
    /// Two 32-bit floats.
    Float2 = 1,
    /// Four 32-bit floats.
    Float4 = 2,
    /// Four 8-bit unsigned normalized integers.
    UByte4Norm = 3,
    /// Four 16-bit unsigned normalized integers.
    UShort4Norm = 4,
}

pub const VERTEX_ATTR_TYPE_COUNT: usize = 5;

// ────────────────────────────────────────────────────────────────────────────
// Static lookup tables — mirrors of the `static constexpr const uint8_t` arrays
// declared inline in `RenderDevice.h`. Length-checked at compile time against
// the corresponding `*_COUNT` constants.
// ────────────────────────────────────────────────────────────────────────────

/// Vertex-shader index for each `Shader` value. Index with `shader.0 as usize`.
pub const VERTEX_FOR_SHADER: [u8; SHADER_COUNT] = [
    0, 0, 0, 1, 2, 2, 2, 3, 4, 4, 4, 4, 5, 6, 6, 6, 7, 8, 8, 8, 8, 9, 10, 10, 10, 11, 12, 12, 12,
    12, 9, 10, 10, 10, 11, 12, 12, 12, 12, 13, 14, 14, 14, 15, 16, 16, 16, 16, 17, 18, 19, 13, 20,
];

/// Vertex-format index for each `VertexShader` value.
pub const FORMAT_FOR_VERTEX: [u8; VERTEX_SHADER_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 9, 10, 11, 12, 13, 10, 14, 15,
];

/// Total vertex stride (bytes) for each `VertexFormat`.
pub const SIZE_FOR_FORMAT: [u8; VERTEX_FORMAT_COUNT] =
    [8, 12, 16, 24, 40, 16, 20, 28, 44, 20, 24, 32, 48, 28, 28, 44];

/// Bitmask of `VertexAttr` values present in each `VertexFormat`.
pub const ATTRIBUTES_FOR_FORMAT: [u8; VERTEX_FORMAT_COUNT] =
    [1, 3, 5, 37, 101, 19, 21, 53, 117, 11, 13, 45, 109, 15, 43, 167];

/// `VertexAttrType` index for each `VertexAttr`.
pub const TYPE_FOR_ATTR: [u8; VERTEX_ATTR_COUNT] = [1, 3, 1, 1, 0, 4, 2, 2];

/// Size in bytes for each `VertexAttrType`.
pub const SIZE_FOR_TYPE: [u8; VERTEX_ATTR_TYPE_COUNT] = [4, 8, 16, 4, 8];

// ────────────────────────────────────────────────────────────────────────────
// Frame primitives — `DeviceCaps`, `Tile`, `UniformData`
// ────────────────────────────────────────────────────────────────────────────

/// Mirror of `Noesis::DeviceCaps`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DeviceCaps {
    /// Offset in pixel units from top-left corner to center of pixel.
    pub center_pixel_offset: f32,
    /// When true, internal textures + offscreens use sRGB; vertex colors are
    /// expected in sRGB, target writes are linear.
    pub linear_rendering: bool,
    /// Device supports LCD subpixel rendering (needs dual-source blending).
    pub subpixel_rendering: bool,
    /// Clip-space depth range is [0, 1] rather than [-1, 1].
    pub depth_range_zero_to_one: bool,
    /// Clip-space Y is inverted (top = -1, bottom = +1).
    pub clip_space_y_inverted: bool,
}

impl Default for DeviceCaps {
    fn default() -> Self {
        // Matches the C++ in-class member initializers.
        Self {
            center_pixel_offset: 0.0,
            linear_rendering: false,
            subpixel_rendering: false,
            depth_range_zero_to_one: true,
            clip_space_y_inverted: false,
        }
    }
}

/// Mirror of `Noesis::Tile` — a region of the render target with origin at
/// the lower-left corner.
#[repr(C)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Mirror of `Noesis::UniformData` — a span of dwords for uniform-buffer
/// updates, plus a content hash so the device can skip redundant uploads.
///
/// `values` points into Noesis-owned memory that lives at least until the
/// `DrawBatch` call returns.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct UniformData {
    /// Pointer to the dword array (may be null when `num_dwords == 0`).
    pub values: *const c_void,
    /// Number of 4-byte dwords at `values`.
    pub num_dwords: u32,
    /// Content hash — equal hashes guarantee equal contents.
    pub hash: u32,
}

impl Default for UniformData {
    fn default() -> Self {
        Self {
            values: core::ptr::null(),
            num_dwords: 0,
            hash: 0,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Layout assertions — these fire at compile time if any mirror drifts from
// the Noesis-side layout. Sizes for the byte-packed types are checked
// explicitly; the `#[repr(C)]` enums get their size from the platform's int
// representation, which already matches Noesis's unscoped enum default.
// ────────────────────────────────────────────────────────────────────────────

const _: () = assert!(size_of::<Shader>() == 1);
const _: () = assert!(align_of::<Shader>() == 1);

const _: () = assert!(size_of::<SamplerState>() == 1);
const _: () = assert!(align_of::<SamplerState>() == 1);

const _: () = assert!(size_of::<RenderState>() == 1);
const _: () = assert!(align_of::<RenderState>() == 1);

const _: () = assert!(size_of::<DeviceCaps>() == 8);
const _: () = assert!(align_of::<DeviceCaps>() == 4);

const _: () = assert!(size_of::<Tile>() == 16);
const _: () = assert!(align_of::<Tile>() == 4);

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<UniformData>() == 16);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(align_of::<UniformData>() == 8);

#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<UniformData>() == 12);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(align_of::<UniformData>() == 4);
