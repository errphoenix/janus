use std::path::Path;

use super::{GlProperty, GpuResource, gl};
use image::{DynamicImage, ImageError, ImageReader};

const TEXTURE_TARGETS: usize = 5;
const TEXTURE_UNITS: usize = 16;
static mut BINDING_POINTS: [[TextureView; TEXTURE_TARGETS]; TEXTURE_UNITS] =
    [[TextureView::null(TextureKind::Dim2D); TEXTURE_TARGETS]; TEXTURE_UNITS];

/// Get the possibly bound [`TextureView`] for the `target` at the given `unit`.
///
/// Note that if the texture has been bound with [`bind_without_meta`], the
/// texture's metadata will not be preserved, other than the OpenGL texture
/// object.
pub const fn get_bound_texture(target: TextureKind, unit: u32) -> Option<TextureView> {
    let bkeep_i = target.bookkeping_index();

    let texture = unsafe { BINDING_POINTS[unit as usize][bkeep_i] };
    if !texture.is_null() {
        Some(texture)
    } else {
        None
    }
}

/// Binds the given `texture` only by OpenGL state.
///
/// This will store a [`TextureView`] without metadata, only preserving the
/// OpenGL texture object `gl_pointer` field.
pub fn bind_without_meta(target: TextureKind, texture: impl Into<TextureKey>, unit: u32) {
    crate::debug_assert_gl!();

    let texture: TextureKey = texture.into();
    let bkeep_i = target.bookkeping_index();
    unsafe {
        if BINDING_POINTS[unit as usize][bkeep_i].gl_pointer != texture.0 {
            gl::BindTextureUnit(unit, texture.0);
        }
    }

    let dummy = TextureView::null(target);
    unsafe {
        BINDING_POINTS[unit as usize][bkeep_i] = dummy;
    }
}

pub fn bind(texture: impl AsTexView, unit: u32) {
    crate::debug_assert_gl!();

    let texture = texture.as_texture_view();
    let target = texture.target_kind();
    let bkeep_i = target.bookkeping_index();

    unsafe {
        if BINDING_POINTS[unit as usize][bkeep_i] != texture {
            gl::BindTextureUnit(unit, texture.gl_pointer);
        }
    }

    unsafe {
        BINDING_POINTS[unit as usize][bkeep_i] = texture;
    }
}

pub fn unbind(target: TextureKind, unit: u32) {
    crate::debug_assert_gl!();

    assert!(unit < TEXTURE_UNITS as u32);
    let bkeep_i = target.bookkeping_index();

    unsafe {
        if !BINDING_POINTS[unit as usize][bkeep_i].is_null() {
            gl::BindTextureUnit(unit, 0);
        }
    }

    unsafe {
        BINDING_POINTS[unit as usize].fill(TextureView::null(TextureKind::Dim2D));
    }
}

#[inline(always)]
fn load_image<P: AsRef<Path>>(path: P) -> Result<DynamicImage, ImageError> {
    ImageReader::open(path)?.with_guessed_format()?.decode()
}

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum TextureError {
    #[error("failed to load image: {0}")]
    ImageLoadError(ImageError),

    #[error("unsupported image format")]
    UnsupportedFormat,
}

#[derive(Debug, Default)]
pub struct Textures {
    inner: Vec<Texture>,
}
impl Textures {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    pub fn from(textures: Vec<Texture>) -> Self {
        Self { inner: textures }
    }

    pub fn owner(&self, index: usize) -> &Texture {
        &self.inner[index]
    }

    pub fn delete(&mut self, index: usize) -> Texture {
        self.inner.remove(index)
    }

    pub fn view(&self, index: usize) -> TextureView {
        self.inner[index].view()
    }

    pub fn put(&mut self, texture: Texture) -> usize {
        let i = self.inner.len();
        self.inner.push(texture);
        i
    }

    pub fn put_and_view(&mut self, texture: Texture) -> TextureView {
        let view = texture.view();
        self.inner.push(texture);
        view
    }
}

/// The owner of an OpenGL texture.
///
/// This contains a pointer to the texture and its [`metadata`](ImageMetadata).
///
/// This *owns* the texture resource on the GPU, so when it is dropped the
/// GPU resource will also be cleared along with it.
///
/// Thus, this must be used for persistent texture storage on the CPU. For most
/// other purposes, you might want to **acquire** a
/// [`view`](TextureView) of this texture.
#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Texture {
    gl_pointer: u32,
    metadata: TextureMetadata,
}
impl Texture {
    pub fn from_2d_image(
        image: &DynamicImage,
        mip_levels: MipLevels,
    ) -> Result<Self, TextureError> {
        let (bytes, w, h, (px, fmt)) = {
            let bytes: Box<[u8]> = image.as_bytes().into();
            let width = image.width() as i32;
            let height = image.height() as i32;

            let (pixel, format) = match image {
                image::DynamicImage::ImageRgb8(_) => Ok((ImageType::Bits8, ImageFormat::Rgb)),
                image::DynamicImage::ImageRgba8(_) => Ok((ImageType::Bits8, ImageFormat::Rgba)),
                image::DynamicImage::ImageRgb16(_) => Ok((ImageType::Bits16, ImageFormat::Rgb)),
                image::DynamicImage::ImageRgba16(_) => Ok((ImageType::Bits16, ImageFormat::Rgba)),
                image::DynamicImage::ImageRgb32F(_) => Ok((ImageType::Float32, ImageFormat::Rgb)),
                image::DynamicImage::ImageRgba32F(_) => Ok((ImageType::Float32, ImageFormat::Rgba)),
                image::DynamicImage::ImageLuma8(_) => {
                    Ok((ImageType::Bits8, ImageFormat::SingleChannel))
                }
                image::DynamicImage::ImageLumaA8(_) => {
                    Ok((ImageType::Bits8, ImageFormat::DualChannel))
                }
                image::DynamicImage::ImageLuma16(_) => {
                    Ok((ImageType::Bits16, ImageFormat::SingleChannel))
                }
                image::DynamicImage::ImageLumaA16(_) => {
                    Ok((ImageType::Bits16, ImageFormat::DualChannel))
                }
                _ => Err(TextureError::UnsupportedFormat),
            }?;

            (bytes, width, height, (pixel, format))
        };

        let texture = Self::new_2d(w, h, mip_levels, px, fmt);
        texture.upload_2d_whole(0, &bytes).expect("texture is 2d");
        Ok(texture)
    }

    pub fn from_2d_image_file(
        path: impl AsRef<Path>,
        mip_levels: MipLevels,
    ) -> Result<Self, TextureError> {
        let image = load_image(path).map_err(TextureError::ImageLoadError)?;
        Self::from_2d_image(&image, mip_levels)
    }

    pub fn new(
        kind: TextureKind,
        width: i32,
        height: i32,
        layers: i32,
        mip_levels: MipLevels,
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        if kind == TextureKind::Dim2D {
            assert_eq!(layers, 1, "2d texture must have exactly one layer");
        } else if kind == TextureKind::CubeMap {
            assert_eq!(layers, 6, "cubemap texture must have exactly 6 layers");
        } else if kind == TextureKind::CubeMapArray {
            assert_eq!(
                layers % 6,
                0,
                "cubemap array texture must have a multiple of 6 layers"
            );
        }

        let gl_format = choose_gl_format(format, pixel);
        let id = create(kind);
        allocate_texture(
            id,
            kind,
            width,
            height,
            layers,
            mip_levels,
            gl_format.internal,
        );

        let mip_levels = mip_levels.get();
        let metadata = TextureMetadata {
            width,
            height,
            format,
            pixel,
            gl_format,
            kind,
            layers,
            mip_levels,
        };

        Self {
            gl_pointer: id,
            metadata,
        }
    }

    pub fn new_2d(
        width: i32,
        height: i32,
        mip_levels: MipLevels,
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        Self::new(
            TextureKind::Dim2D,
            width,
            height,
            1,
            mip_levels,
            pixel,
            format,
        )
    }

    pub fn new_array(
        width: i32,
        height: i32,
        layers: i32,
        mip_levels: MipLevels,
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        Self::new(
            TextureKind::Dim2DArray,
            width,
            height,
            layers,
            mip_levels,
            pixel,
            format,
        )
    }

    pub fn new_3d(
        width: i32,
        height: i32,
        depth: i32,
        mip_levels: MipLevels,
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        Self::new(
            TextureKind::Dim3D,
            width,
            height,
            depth,
            mip_levels,
            pixel,
            format,
        )
    }

    pub fn new_cubemap(
        width: i32,
        height: i32,
        mip_levels: MipLevels,
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        Self::new(
            TextureKind::CubeMap,
            width,
            height,
            6,
            mip_levels,
            pixel,
            format,
        )
    }

    pub fn new_cubemap_array(
        width: i32,
        height: i32,
        num_cubemaps: i32,
        mip_levels: MipLevels,
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        assert!(num_cubemaps > 0);
        Self::new(
            TextureKind::CubeMapArray,
            width,
            height,
            num_cubemaps * 6,
            mip_levels,
            pixel,
            format,
        )
    }

    pub const fn view(&self) -> TextureView {
        TextureView {
            gl_pointer: self.gl_pointer,
            metadata: self.metadata,
        }
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        let ptr = self.gl_pointer;
        unsafe {
            gl::DeleteTextures(1, &ptr);
        }
    }
}
impl GpuResource for Texture {
    fn resource_id(&self) -> u32 {
        self.gl_pointer
    }
}
impl Tex for Texture {
    fn target_kind(&self) -> TextureKind {
        self.metadata.kind
    }

    fn metadata(&self) -> TextureMetadata {
        self.metadata
    }
}

/// A direct abstraction of an OpenGL texture object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureKey(pub u32);
impl From<TextureView> for TextureKey {
    fn from(value: TextureView) -> Self {
        Self(value.resource_id())
    }
}
impl From<Texture> for TextureKey {
    fn from(value: Texture) -> Self {
        Self(value.resource_id())
    }
}

/// A reference to an OpenGL texture.
///
/// This is different from [`Texture`] because this does not actually own
/// the texture resources on the GPU and it has no effects on its lifecycle.
///
/// Internally, [`TextureView`] holds a copy to the u32 OpenGL pointer to the
/// texture resource, thus allowing OpenGL operations.
#[derive(Clone, Debug, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct TextureView {
    gl_pointer: u32,
    metadata: TextureMetadata,
}
impl TextureView {
    pub const fn null(kind: TextureKind) -> Self {
        Self {
            gl_pointer: 0,
            metadata: TextureMetadata::null(kind),
        }
    }

    pub const fn is_null(&self) -> bool {
        self.gl_pointer == 0
    }

    pub const fn of(texture: &Texture) -> Self {
        Self {
            gl_pointer: texture.gl_pointer,
            metadata: texture.metadata,
        }
    }
}
impl Tex for TextureView {
    fn target_kind(&self) -> TextureKind {
        self.metadata.kind
    }

    fn metadata(&self) -> TextureMetadata {
        self.metadata
    }
}
impl GpuResource for TextureView {
    fn resource_id(&self) -> u32 {
        self.gl_pointer
    }
}
impl From<&'_ Texture> for TextureView {
    fn from(value: &'_ Texture) -> Self {
        value.view()
    }
}

pub trait AsTexView {
    fn as_texture_view(&self) -> TextureView;
}
impl AsTexView for TextureView {
    fn as_texture_view(&self) -> TextureView {
        *self
    }
}
impl AsTexView for Texture {
    fn as_texture_view(&self) -> TextureView {
        self.view()
    }
}

pub trait Tex: GpuResource + AsTexView {
    fn target_kind(&self) -> TextureKind;
    fn metadata(&self) -> TextureMetadata;

    fn size(&self) -> (i32, i32) {
        (self.metadata().width, self.metadata().height)
    }

    fn texture_id(&self) -> u32 {
        self.resource_id()
    }

    fn is_null(&self) -> bool {
        self.texture_id() == 0
    }

    fn bind(&self, unit: u32) {
        bind(self.as_texture_view(), unit);
    }

    fn unbind(&self, unit: u32) {
        unbind(self.target_kind(), unit);
    }

    fn is_bound(&self, unit: u32) -> bool {
        get_bound_texture(self.target_kind(), unit)
            .is_some_and(|t| t.gl_pointer == self.texture_id())
    }

    fn set_gl_parameteri(&self, gl_parameter: u32, value: i32) {
        let texture = self.texture_id();
        unsafe {
            gl::TextureParameteri(texture, gl_parameter, value);
        }
    }

    fn set_gl_parameterf(&self, gl_parameter: u32, value: f32) {
        let texture = self.texture_id();
        unsafe {
            gl::TextureParameterf(texture, gl_parameter, value);
        }
    }

    fn set_filtering_min(&self, filtering: TextureFiltering) {
        let value = filtering.force_base_filtering().property_enum();
        self.set_gl_parameteri(gl::TEXTURE_MIN_FILTER, value as i32);
    }

    fn set_filtering_mag(&self, filtering: TextureFiltering) {
        let value = filtering.property_enum();
        self.set_gl_parameteri(gl::TEXTURE_MAG_FILTER, value as i32);
    }

    fn set_filtering_minmag(&self, filtering: TextureFiltering) {
        self.set_filtering_min(filtering);
        self.set_filtering_mag(filtering);
    }

    fn set_wrapping_s(&self, wrapping: TextureWrapping) {
        let value = wrapping.property_enum();
        self.set_gl_parameteri(gl::TEXTURE_WRAP_S, value as i32);
    }

    fn set_wrapping_t(&self, wrapping: TextureWrapping) {
        let value = wrapping.property_enum();
        self.set_gl_parameteri(gl::TEXTURE_WRAP_T, value as i32);
    }

    fn set_wrapping_r(&self, wrapping: TextureWrapping) {
        let value = wrapping.property_enum();
        self.set_gl_parameteri(gl::TEXTURE_WRAP_R, value as i32);
    }

    fn set_wrapping_st(&self, wrapping: TextureWrapping) {
        self.set_wrapping_s(wrapping);
        self.set_wrapping_t(wrapping);
    }

    fn set_wrapping_str(&self, wrapping: TextureWrapping) {
        self.set_wrapping_st(wrapping);
        self.set_wrapping_r(wrapping);
    }

    fn upload_slice(
        &self,
        mip_level: i32,
        x: i32,
        y: i32,
        z: i32,
        w: i32,
        h: i32,
        d: i32,
        data: &[u8],
    ) -> Result<(), TextureUploadParamsError> {
        let meta = self.metadata();
        let x = x.min(meta.width);
        let y = y.min(meta.height);

        // this is done before clamping z, because an invalid layer index or
        // span may indicate an serious mismatch between the caller's belief
        // of the type of the texture and the actual texture type.
        match self.target_kind() {
            TextureKind::Dim2D if z != 0 || d != 1 => {
                return Err(TextureUploadParamsError::InvalidLayerIndex2d(z + d));
            }
            TextureKind::CubeMap if z + d > 6 => {
                return Err(TextureUploadParamsError::InvalidLayerIndexCubemap(z + d));
            }
            _ => {}
        }

        let z = z.min(meta.layers);

        upload_texture(
            self.texture_id(),
            self.target_kind(),
            mip_level,
            x,
            y,
            w,
            h,
            z,
            d,
            data,
            self.metadata().gl_format,
        );

        Ok(())
    }

    fn upload_2d(
        &self,
        mip_level: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        data: &[u8],
    ) -> Result<(), TextureUploadParamsError> {
        self.upload_slice(mip_level, x, y, 0, w, h, 1, data)
    }

    fn upload_2d_whole(&self, mip_level: i32, data: &[u8]) -> Result<(), TextureUploadParamsError> {
        let (w, h) = self.size();
        self.upload_2d(mip_level, 0, 0, w, h, data)
    }

    fn upload_layer(
        &self,
        mip_level: i32,
        x: i32,
        y: i32,
        layer: i32,
        w: i32,
        h: i32,
        data: &[u8],
    ) -> Result<(), TextureUploadParamsError> {
        self.upload_slice(mip_level, x, y, layer, w, h, 1, data)
    }

    fn upload_layer_whole(
        &self,
        mip_level: i32,
        layer: i32,
        data: &[u8],
    ) -> Result<(), TextureUploadParamsError> {
        let (w, h) = self.size();
        self.upload_slice(mip_level, 0, 0, layer, w, h, 1, data)
    }

    fn upload_cubemap_array_face(
        &self,
        mip_level: i32,
        cubemap_index: i32,
        x: i32,
        y: i32,
        face: i32,
        w: i32,
        h: i32,
        data: &[u8],
    ) -> Result<(), TextureUploadParamsError> {
        if face > 5 {
            return Err(TextureUploadParamsError::InvalidLayerFaceIndexCubemapArray(
                face,
            ));
        }
        let layer = (cubemap_index * 6) + face;
        self.upload_layer(mip_level, x, y, layer, w, h, data)
    }

    fn upload_cubemap_array_face_whole(
        &self,
        mip_level: i32,
        cubemap_index: i32,
        face: i32,
        data: &[u8],
    ) -> Result<(), TextureUploadParamsError> {
        let (w, h) = self.size();
        self.upload_cubemap_array_face(mip_level, cubemap_index, 0, 0, face, w, h, data)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TextureUploadParamsError {
    #[error("target is strictly 2D, but an invalid layer index or span ({0} != 1) was provided")]
    InvalidLayerIndex2d(i32),
    #[error("target is strictly Cubemap, but an invalid layer index + span ({0} > 6) was provided")]
    InvalidLayerIndexCubemap(i32),
    #[error("target is a Cubemap Array, but an invalid layer FACE index ({0} > 5) was provided")]
    InvalidLayerFaceIndexCubemapArray(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum TextureKind {
    Dim2D,
    Dim2DArray,
    Dim3D,
    CubeMap,
    CubeMapArray,
}
impl TextureKind {
    const fn bookkeping_index(self) -> usize {
        match self {
            TextureKind::Dim2D => 0,
            TextureKind::Dim2DArray => 1,
            TextureKind::Dim3D => 2,
            TextureKind::CubeMap => 3,
            TextureKind::CubeMapArray => 4,
        }
    }
}
impl GlProperty for TextureKind {
    fn property_enum(self) -> u32 {
        match self {
            TextureKind::Dim2D => gl::TEXTURE_2D,
            TextureKind::Dim2DArray => gl::TEXTURE_2D_ARRAY,
            TextureKind::Dim3D => gl::TEXTURE_3D,
            TextureKind::CubeMap => gl::TEXTURE_CUBE_MAP,
            TextureKind::CubeMapArray => gl::TEXTURE_CUBE_MAP_ARRAY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct TextureMetadata {
    kind: TextureKind,
    width: i32,
    height: i32,
    layers: i32,
    mip_levels: i32,
    format: ImageFormat,
    pixel: ImageType,
    gl_format: GlFormat,
}
impl TextureMetadata {
    const fn null(kind: TextureKind) -> Self {
        Self {
            kind,
            width: 1,
            height: 1,
            layers: 1,
            mip_levels: 1,
            format: ImageFormat::Rgba,
            pixel: ImageType::Bits8,
            gl_format: GlFormat {
                internal: 0,
                format: 0,
                data_type: 0,
            },
        }
    }

    pub const fn kind(&self) -> TextureKind {
        self.kind
    }

    /// Returns the largest side of the texture (between width and height).
    pub fn max_size(&self) -> i32 {
        self.width.max(self.height)
    }

    pub const fn width(&self) -> i32 {
        self.width
    }

    pub const fn height(&self) -> i32 {
        self.height
    }

    pub const fn layers(&self) -> i32 {
        match self.kind {
            TextureKind::Dim2D => 1,
            TextureKind::CubeMap => 6,
            TextureKind::Dim2DArray | TextureKind::Dim3D | TextureKind::CubeMapArray => self.layers,
        }
    }

    pub const fn mip_levels(&self) -> i32 {
        self.mip_levels
    }

    pub const fn format(&self) -> ImageFormat {
        self.format
    }

    pub const fn pixel(&self) -> ImageType {
        self.pixel
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, Default)]
pub enum TextureFiltering {
    Linear,

    #[default]
    Nearest,

    /// The inner bool indicates whether the mipmap's inner filtering is
    /// linear or not.
    NearestMipmap(bool),
    /// The inner bool indicates whether the mipmap's inner filtering is
    /// linear or not.
    LinearMipmap(bool),
}
impl GlProperty for TextureFiltering {
    fn property_enum(self) -> u32 {
        match self {
            TextureFiltering::Linear => gl::LINEAR,
            TextureFiltering::Nearest => gl::NEAREST,
            TextureFiltering::NearestMipmap(true) => gl::NEAREST_MIPMAP_LINEAR,
            TextureFiltering::NearestMipmap(false) => gl::NEAREST_MIPMAP_NEAREST,
            TextureFiltering::LinearMipmap(true) => gl::LINEAR_MIPMAP_LINEAR,
            TextureFiltering::LinearMipmap(false) => gl::LINEAR_MIPMAP_NEAREST,
        }
    }
}
impl TextureFiltering {
    /// Convers mipmap filtering to a "base" filter, this being either
    /// [`TextureFiltering::Linear`] or [`TextureFiltering::Nearest`].
    ///
    /// This depends on the `bool` value, which indicates whether the inner
    /// mipmap filtering is linear or not.
    ///
    /// This is useful to coerce a mipmap texture filtering option for
    /// `GL_TEXTURE_MIN_FILTER` to a valid option for `GL_TEXTURE_MAG_FILTER`,
    /// since the latter does not permit mipmmaps.
    const fn force_base_filtering(self) -> TextureFiltering {
        match self {
            TextureFiltering::NearestMipmap(linear) => {
                if linear {
                    TextureFiltering::Linear
                } else {
                    TextureFiltering::Nearest
                }
            }
            TextureFiltering::LinearMipmap(linear) => {
                if linear {
                    TextureFiltering::Linear
                } else {
                    TextureFiltering::Nearest
                }
            }
            base_filtering => base_filtering,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, Default)]
pub enum TextureWrapping {
    #[default]
    Clamp,
    Repeat,
    Mirrored,
}
impl GlProperty for TextureWrapping {
    fn property_enum(self) -> u32 {
        match self {
            TextureWrapping::Clamp => gl::CLAMP_TO_EDGE,
            TextureWrapping::Repeat => gl::REPEAT,
            TextureWrapping::Mirrored => gl::MIRRORED_REPEAT,
        }
    }
}

/// The image format for an OpenGL texture.
///
/// This determines to the number of components present in the texture for
/// each pixel.
///
/// This directly corresponds to the `format` field in OpenGL `glTexImageXD`
/// function calls.
/// It is also used to determine which `internalformat` should be used,
/// together with [`ImageType`].
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum ImageFormat {
    SingleChannel,
    DualChannel,
    Rgb,
    Bgr,
    Rgba,
    Bgra,
    SingleChannelInteger,
    DualChannelInteger,
    RgbInteger,
    BgrInteger,
    RgbaInteger,
    BgraInteger,
    Depth,
    Stencil,
    DepthStencil,
    RgbSnorm8,
    RgbaSnorm8,
    RgbSnorm16,
    RgbaSnorm16,
}
impl GlProperty for ImageFormat {
    fn property_enum(self) -> u32 {
        self.to_gl_format()
    }
}
impl ImageFormat {
    pub const fn has_alpha(&self) -> bool {
        use ImageFormat::*;
        matches!(
            self,
            Rgba | Bgra | RgbaInteger | BgraInteger | RgbaSnorm8 | RgbaSnorm16
        )
    }

    pub const fn to_gl_format(self) -> u32 {
        use ImageFormat::*;

        match self {
            SingleChannel => gl::RED,
            DualChannel => gl::RG,
            Rgb => gl::RGB,
            Rgba => gl::RGBA,
            Bgr => gl::BGR,
            Bgra => gl::BGRA,

            RgbSnorm8 => gl::RGB8_SNORM,
            RgbaSnorm8 => gl::RGBA8_SNORM,
            RgbSnorm16 => gl::RGB16_SNORM,
            RgbaSnorm16 => gl::RGBA16_SNORM,

            SingleChannelInteger => gl::RED_INTEGER,
            DualChannelInteger => gl::RG_INTEGER,
            RgbInteger => gl::RGB_INTEGER,
            RgbaInteger => gl::RGBA_INTEGER,
            BgrInteger => gl::BGR_INTEGER,
            BgraInteger => gl::BGRA_INTEGER,

            Depth => gl::DEPTH_COMPONENT,
            Stencil => gl::STENCIL_INDEX,
            DepthStencil => gl::DEPTH_STENCIL,
        }
    }
}

/// The data type of the pixels in an OpenGL texture.
///
/// This, together with [`ImageFormat`], determines the `internalformat` field
/// for OpenGL `glTexImageXD` function calls.
/// It also determines the value for the `type` field.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum ImageType {
    /// Only works with a [`stencil index attachment`](ImageFormat::Stencil)
    /// for the equivalent to `GL_STENCIL_INDEX1`.
    SingleBit,

    /// Works only for the [`RGB`](ImageFormat::Rgb) image format.
    ///
    /// The C OpenGL equivalent is `GL_R3_G3_B2`.
    Bits332,
    Bits4,
    /// If alpha is present (i.e. the format is either [`ImageFormat::Rgb`]
    /// or [`ImageFormat::Bgra`]), then the Alpha component will be 1 bit.
    ///
    /// In that case, the C OpenGL equivalent would be `GL_RGB5_A1`.
    Bits5,
    Bits8,
    /// If alpha is present (i.e. the format is either [`ImageFormat::Rgb`]
    /// or [`ImageFormat::Bgra`]), then the Alpha component will be 2 bits.
    ///
    /// In that case, the C OpenGL equivalent would be `GL_RGB10_A2`.
    Bits10,
    Bits12,
    Bits16,

    /// Only used for [`depth component`](ImageFormat::Depth) and
    /// [`depth + stencil`](ImageFormat::DepthStencil) for the equivalents to
    /// `GL_DEPTH_COMPONENT24` and `GL_DEPTH24_STENCIL8`.
    Bits24,

    /// Represents a color (RGBA) packet into a single 8-bits integer (1 byte)
    /// with each component being represented by 2 bits.
    ///
    /// Essentially the C OpenGL equivalent to `GL_RGBA2`.
    ///
    /// This requires the [`RGB image format`](ImageFormat::Rgb)
    /// (or [`BGR`](ImageFormat::Bgr)).
    Bits2PackedByte1,
    /// Represents a color (RGBA) packet into a 16-bits integer (2 bytes)
    /// with each component being represented by 4 bits.
    ///
    /// Essentially the C OpenGL equivalent to `GL_RGBA4`.
    ///
    /// This requires the [`RGB image format`](ImageFormat::Rgb)
    /// (or [`BGR`](ImageFormat::Bgr)).
    Bits4PackedByte2,

    Bits8Snorm,
    Bits16Snorm,

    Bits8Linear,

    /// Works only for the [`RGB`](ImageFormat::Rgb) image format.
    ///
    /// The C OpenGL equivalent is `GL_RGB9_E5`.
    Bits9Shared5,

    Float16,
    Float32,
    /// Works only for the [`RGB`](ImageFormat::Rgb) image format.
    ///
    /// The C OpenGL equivalent is `GL_R11F_G11F_B10F`.
    Float111110,

    Integer8,
    Integer16,
    Integer32,

    Integer8U,
    Integer16U,
    Integer32U,
}
impl GlProperty for ImageType {
    fn property_enum(self) -> u32 {
        self.to_gl_type(false)
    }
}
impl ImageType {
    pub const fn to_gl_type(self, alpha: bool) -> u32 {
        use ImageType::*;

        match self {
            Bits5 if alpha => gl::UNSIGNED_SHORT_5_5_5_1,
            Bits10 if alpha => gl::UNSIGNED_INT_10_10_10_2,

            Bits332 => gl::UNSIGNED_BYTE_3_3_2,
            SingleBit | Bits2PackedByte1 | Bits4PackedByte2 | Bits8Linear | Bits4 | Bits5
            | Bits8 => gl::UNSIGNED_BYTE,

            Bits16Snorm | Bits8Snorm => gl::BYTE,

            Bits16 | Bits12 => gl::UNSIGNED_SHORT,
            Bits10 | Bits24 => gl::UNSIGNED_INT,
            Bits9Shared5 => gl::UNSIGNED_INT_5_9_9_9_REV,

            Float16 | Float32 | Float111110 => gl::FLOAT,

            Integer8 | Integer16 | Integer32 => gl::INT,
            Integer8U | Integer16U | Integer32U => gl::UNSIGNED_INT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
struct GlFormat {
    internal: u32,
    format: u32,
    data_type: u32,
}

fn choose_gl_format(format: ImageFormat, pixel: ImageType) -> GlFormat {
    use ImageType::*;

    fn invalid(pixel: ImageType, format: ImageFormat) -> ! {
        panic!("attempted to use invalid pixel type {pixel:?} for format {format:?}");
    }

    let internal = match format {
        ImageFormat::SingleChannel => match pixel {
            Bits8 => gl::R8,
            Bits16 => gl::R16,
            Bits8Snorm => gl::R8_SNORM,
            Bits16Snorm => gl::R16_SNORM,
            Float16 => gl::R16F,
            Float32 => gl::R32F,
            wrong => invalid(wrong, format),
        },
        ImageFormat::DualChannel => match pixel {
            Bits8 => gl::RG8,
            Bits16 => gl::RG16,
            Bits8Snorm => gl::RG8_SNORM,
            Bits16Snorm => gl::RG16_SNORM,
            Float16 => gl::RG16F,
            Float32 => gl::RG32F,
            wrong => invalid(wrong, format),
        },
        ImageFormat::Rgb | ImageFormat::Bgr => match pixel {
            Bits332 => gl::R3_G3_B2,
            Bits4 => gl::RGB4,
            Bits5 => gl::RGB5,
            Bits8 => gl::RGB8,
            Bits16 => gl::RGB16,
            Bits8Snorm => gl::RGB8_SNORM,
            Bits10 => gl::RGB10,
            Bits12 => gl::RGB12,
            Bits16Snorm => gl::RGB16_SNORM,
            Bits8Linear => gl::SRGB8,
            Float16 => gl::RGB16F,
            Float32 => gl::RGB32F,
            Float111110 => gl::R11F_G11F_B10F,
            Bits2PackedByte1 => gl::RGBA2,
            Bits4PackedByte2 => gl::RGBA4,
            Bits9Shared5 => gl::RGB9_E5,
            wrong => invalid(wrong, format),
        },
        ImageFormat::Rgba | ImageFormat::Bgra => match pixel {
            Bits5 => gl::RGB5_A1,
            Bits8 => gl::RGBA8,
            Bits8Snorm => gl::RGBA8_SNORM,
            Bits10 => gl::RGB10_A2,
            Bits12 => gl::RGBA12,
            Bits16 => gl::RGBA16,
            Bits8Linear => gl::SRGB8_ALPHA8,
            Bits16Snorm => gl::RGBA16_SNORM,
            Float16 => gl::RGBA16F,
            Float32 => gl::RGBA32F,
            wrong => invalid(wrong, format),
        },

        ImageFormat::SingleChannelInteger => match pixel {
            Integer8 => gl::R8I,
            Integer16 => gl::R16I,
            Integer32 => gl::R32I,
            Integer8U => gl::R8UI,
            Integer16U => gl::R16UI,
            Integer32U => gl::R32UI,
            wrong => invalid(wrong, format),
        },
        ImageFormat::DualChannelInteger => match pixel {
            Integer8 => gl::RG8I,
            Integer16 => gl::RG16I,
            Integer32 => gl::RG32I,
            Integer8U => gl::RG8UI,
            Integer16U => gl::RG16UI,
            Integer32U => gl::RG32UI,
            wrong => invalid(wrong, format),
        },
        ImageFormat::RgbInteger | ImageFormat::BgrInteger => match pixel {
            Integer8 => gl::RGB8I,
            Integer16 => gl::RGB16I,
            Integer32 => gl::RGB32I,
            Integer8U => gl::RGB8UI,
            Integer16U => gl::RGB16UI,
            Integer32U => gl::RGB32UI,
            wrong => invalid(wrong, format),
        },
        ImageFormat::RgbaInteger | ImageFormat::BgraInteger => match pixel {
            Integer8 => gl::RGBA8I,
            Integer16 => gl::RGBA16I,
            Integer32 => gl::RGBA32I,
            Integer8U => gl::RGBA8UI,
            Integer16U => gl::RGBA16UI,
            Integer32U => gl::RGBA32UI,
            wrong => invalid(wrong, format),
        },

        ImageFormat::Depth => match pixel {
            Bits16 => gl::DEPTH_COMPONENT16,
            Bits24 => gl::DEPTH_COMPONENT24,
            Integer32 => gl::DEPTH_COMPONENT32,
            Float32 => gl::DEPTH_COMPONENT32F,
            wrong => invalid(wrong, format),
        },
        ImageFormat::Stencil => match pixel {
            SingleBit => gl::STENCIL_INDEX1,
            Bits4 => gl::STENCIL_INDEX4,
            Bits8 => gl::STENCIL_INDEX8,
            Bits16 => gl::STENCIL_INDEX16,
            wrong => invalid(wrong, format),
        },
        ImageFormat::DepthStencil => match pixel {
            Bits24 => gl::DEPTH24_STENCIL8,
            Float32 => gl::DEPTH32F_STENCIL8,
            wrong => invalid(wrong, format),
        },
        ImageFormat::RgbaSnorm8 => match pixel {
            Bits8Snorm => gl::RGBA8_SNORM,
            wrong => invalid(wrong, format),
        },
        ImageFormat::RgbSnorm8 => match pixel {
            Bits8Snorm => gl::RGB8_SNORM,
            wrong => invalid(wrong, format),
        },
        ImageFormat::RgbaSnorm16 => match pixel {
            Bits16Snorm => gl::RGBA16_SNORM,
            wrong => invalid(wrong, format),
        },
        ImageFormat::RgbSnorm16 => match pixel {
            Bits16Snorm => gl::RGB16_SNORM,
            wrong => invalid(wrong, format),
        },
    };

    GlFormat {
        internal,
        format: format.to_gl_format(),
        data_type: pixel.to_gl_type(format.has_alpha()),
    }
}

/// Texture Mip-levels amount. Must be atleast 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MipLevels(std::num::NonZeroI32);
impl Default for MipLevels {
    fn default() -> Self {
        Self::try_new(1).expect("1 is not 0.")
    }
}
impl MipLevels {
    pub const fn new(mip_count: std::num::NonZeroI32) -> Self {
        Self(mip_count)
    }

    pub fn try_new(mip_count: i32) -> Option<Self> {
        std::num::NonZeroI32::new(mip_count).map(Self::new)
    }

    pub const fn get(&self) -> i32 {
        self.0.get()
    }
}

fn create(kind: TextureKind) -> u32 {
    let target = kind.property_enum();
    let mut id = 0;
    unsafe {
        gl::CreateTextures(target, 1, &mut id);
    }
    id
}

fn allocate_texture(
    texture: u32,
    kind: TextureKind,
    width: i32,
    height: i32,
    layers: i32,
    mip_levels: MipLevels,
    internal_format: u32,
) {
    let mip_levels = mip_levels.get();
    match kind {
        TextureKind::Dim2D => unsafe {
            gl::TextureStorage2D(texture, mip_levels, internal_format, width, height);
        },
        TextureKind::Dim2DArray | TextureKind::Dim3D => unsafe {
            gl::TextureStorage3D(texture, mip_levels, internal_format, width, height, layers);
        },
        TextureKind::CubeMap => {
            assert_eq!(
                layers, 6,
                "cubemap texture allocation must provide exactly 6 layers"
            );
            unsafe {
                gl::TextureStorage2D(texture, mip_levels, internal_format, width, height);
            }
        }
        TextureKind::CubeMapArray => {
            assert_eq!(
                layers % 6,
                0,
                "cubemap array texture allocation must provide a multiple of 6 layers"
            );
            unsafe {
                gl::TextureStorage3D(texture, mip_levels, internal_format, width, height, layers);
            }
        }
    }
}

fn upload_texture(
    texture: u32,
    kind: TextureKind,
    mip_level: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    layer_offset: i32,
    layer_span: i32,
    data: &[u8],
    format: GlFormat,
) {
    let data_type = format.data_type;
    let format = format.format;

    match kind {
        TextureKind::Dim2D => unsafe {
            gl::TextureSubImage2D(
                texture,
                mip_level,
                x,
                y,
                width,
                height,
                format,
                data_type,
                data.as_ptr() as *const _,
            );
        },
        TextureKind::Dim2DArray | TextureKind::Dim3D | TextureKind::CubeMapArray => {
            assert_ne!(
                layer_span, 0,
                "upload to a 3d texture or texture array must span over atleast one layer"
            );
            unsafe {
                gl::TextureSubImage3D(
                    texture,
                    mip_level,
                    x,
                    y,
                    layer_offset,
                    width,
                    height,
                    layer_span,
                    format,
                    data_type,
                    data.as_ptr() as *const _,
                );
            }
        }
        TextureKind::CubeMap => {
            assert!(
                layer_offset >= 0,
                "upload to cubemap cannot offset from a negative layer"
            );
            assert!(
                layer_offset + layer_span <= 6,
                "upload to cubemap cannot span out of bounds (a cubemap has exactly 6 layers)"
            );
            unsafe {
                gl::TextureSubImage3D(
                    texture,
                    mip_level,
                    x,
                    y,
                    layer_offset,
                    width,
                    height,
                    layer_span,
                    format,
                    data_type,
                    data.as_ptr() as *const _,
                );
            }
        }
    }
}

/// Sets *global* min and mag filtering to the given `filtering`.
///
/// The mag filter is converted to the filtering "base type" (either nearest
/// or linear) with [`TextureFiltering::force_base_filtering`].
///
/// These correspond to the `GL_TEXTURE_MIN_FILTER` and `GL_TEXTURE_MAG_FILTER`
/// C OpenGL enums to set texture parameters.
pub fn set_filter(target: TextureKind, filtering: TextureFiltering) {
    let target = target.property_enum();
    let mag_filtering = filtering.force_base_filtering().property_enum();
    let min_filtering = filtering.property_enum();

    unsafe {
        gl::TexParameteri(target, gl::TEXTURE_MIN_FILTER, min_filtering as i32);
        gl::TexParameteri(target, gl::TEXTURE_MAG_FILTER, mag_filtering as i32);
    }
}

/// Set *global* ST texture wrapping for 2D textures.
///
/// These correspond to the `GL_TEXTURE_WRAP_S` and `GL_TEXTURE_WRAP_T` C
/// OpenGL enums to set texture parameters.
pub fn set_wrapping_st(target: TextureKind, wrapping: TextureWrapping) {
    let target = target.property_enum();
    let wrapping = wrapping.property_enum();

    unsafe {
        gl::TexParameteri(target, gl::TEXTURE_WRAP_S, wrapping as i32);
        gl::TexParameteri(target, gl::TEXTURE_WRAP_T, wrapping as i32);
    }
}

/// Set *global* R texture wrapping used in 3D textures.
///
/// It is meant to be used in combination with [`set_wrapping_st`].
///
/// This corresponds to the `GL_TEXTURE_WRAP_R` C OpenGL enum to set the
/// texture parameter.
pub fn set_wrapping_r(target: TextureKind, wrapping: TextureWrapping) {
    let wrapping = wrapping.property_enum();
    unsafe {
        gl::TexParameteri(target.property_enum(), gl::TEXTURE_WRAP_R, wrapping as i32);
    }
}
