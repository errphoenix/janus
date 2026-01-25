use std::path::Path;

use super::{GlProperty, GpuResource, gl};
use anyhow::Result;
use image::{DynamicImage, ImageReader};

#[inline(always)]
fn load_image<P: AsRef<Path>>(path: P) -> Result<DynamicImage> {
    Ok(ImageReader::open(path)?.with_guessed_format()?.decode()?)
}

#[inline(always)]
fn read_image_data<P: AsRef<Path>>(path: P) -> Result<Box<[u8]>> {
    let decoded = load_image(path)?;
    Ok(decoded.as_bytes().into())
}

#[derive(thiserror::Error, Debug)]
pub enum TextureError {
    #[error("unsupported image format: {0:?}")]
    UnsupportedFormat(image::DynamicImage),
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
    pub metadata: ImageMetadata,
}

impl Texture {
    pub fn from_image(image: DynamicImage) -> Result<Self> {
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
                unsupported => Err(TextureError::UnsupportedFormat(unsupported)),
            }?;

            (bytes, width, height, (pixel, format))
        };

        Ok(Self::from_bytes(w, h, &bytes, px, fmt))
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let image = load_image(path)?;
        Self::from_image(image)
    }

    pub fn from_bytes(
        width: i32,
        height: i32,
        bytes: &[u8],
        pixel: ImageType,
        format: ImageFormat,
    ) -> Self {
        let gl_format = choose_gl_format(format, pixel);
        let id = create();
        upload_bytes_2d(id, width, height, bytes, gl_format);

        Self {
            gl_pointer: id,
            metadata: ImageMetadata {
                width,
                height,
                format,
                pixel,
            },
        }
    }

    pub fn view(&self) -> TextureView {
        TextureView {
            gl_pointer: self.gl_pointer,
        }
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

impl GpuResource for TextureView {
    fn resource_id(&self) -> u32 {
        self.gl_pointer
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, Default)]
pub enum TextureTarget {
    #[default]
    Flat,
    Cube,
    Array,
}

impl GlProperty for TextureTarget {
    fn property_enum(self) -> u32 {
        match self {
            TextureTarget::Flat => gl::TEXTURE_2D,
            TextureTarget::Cube => gl::TEXTURE_3D,
            TextureTarget::Array => gl::TEXTURE_2D_ARRAY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct ImageMetadata {
    width: i32,
    height: i32,
    format: ImageFormat,
    pixel: ImageType,
}

impl ImageMetadata {
    /// Returns the largest side of the texture.
    pub fn max_size(&self) -> i32 {
        self.width.max(self.height)
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn pixel(&self) -> ImageType {
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
    fn force_base_filtering(self) -> TextureFiltering {
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
}

impl GlProperty for ImageFormat {
    fn property_enum(self) -> u32 {
        self.to_gl_format()
    }
}

impl ImageFormat {
    pub fn has_alpha(&self) -> bool {
        use ImageFormat::*;
        matches!(self, Rgba | Bgra | RgbaInteger | BgraInteger)
    }

    pub fn to_gl_format(self) -> u32 {
        use ImageFormat::*;

        match self {
            SingleChannel => gl::RED,
            DualChannel => gl::RG,
            Rgb => gl::RGB,
            Rgba => gl::RGBA,
            Bgr => gl::BGR,
            Bgra => gl::BGRA,

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
    pub fn to_gl_type(self, alpha: bool) -> u32 {
        use ImageType::*;

        match self {
            Bits5 if alpha => gl::UNSIGNED_SHORT_5_5_5_1,
            Bits10 if alpha => gl::UNSIGNED_INT_10_10_10_2,

            Bits332 => gl::UNSIGNED_BYTE_3_3_2,
            SingleBit | Bits2PackedByte1 | Bits4PackedByte2 | Bits8Linear | Bits8Snorm | Bits4
            | Bits5 | Bits8 => gl::UNSIGNED_BYTE,

            Bits16Snorm | Bits16 | Bits12 => gl::UNSIGNED_SHORT,
            Bits10 | Bits24 => gl::UNSIGNED_INT,
            Bits9Shared5 => gl::UNSIGNED_INT_5_9_9_9_REV,

            Float16 | Float32 | Float111110 => gl::FLOAT,

            Integer8 | Integer16 | Integer32 => gl::INT,
            Integer8U | Integer16U | Integer32U => gl::UNSIGNED_INT,
        }
    }
}

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
    };

    GlFormat {
        internal,
        format: format.to_gl_format(),
        data_type: pixel.to_gl_type(format.has_alpha()),
    }
}

fn create() -> u32 {
    let mut id = 0;
    unsafe {
        gl::GenTextures(1, &mut id);
    }
    id
}

/// Uploads a 2D texture to the GPU using `glTexImage2D`.
///
/// After upload the texture is not unbound, allowing the caller to set
/// parameters using `glTexParameterX` right after this call without having
/// to re-bind the texture.
fn upload_bytes_2d(pointer: u32, width: i32, height: i32, data: &[u8], format: GlFormat) {
    let internal = format.internal;
    let data_type = format.data_type;
    let format = format.format;

    unsafe {
        gl::BindTexture(gl::TEXTURE_2D, pointer);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            internal as i32,
            width,
            height,
            0,
            format,
            data_type,
            data.as_ptr().cast(),
        );
    }
}

fn sub_upload_bytes_2d(
    pointer: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    data: &[u8],
    format: u32,
    data_type: u32,
) {
    unsafe {
        gl::BindTexture(gl::TEXTURE_2D, pointer);
        gl::TexSubImage2D(
            gl::TEXTURE_2D,
            0,
            x,
            y,
            w,
            h,
            format,
            data_type,
            data.as_ptr().cast(),
        );
    }
}

/// Allocates a 2D texture to the GPU using `glTexStorage2D`.
///
/// After allocation the texture is not unbound, allowing the caller to set
/// parameters using `glTexParameterX` right after this call without having
/// to re-bind the texture.
fn alloc_2d(pointer: u32, width: i32, height: i32, format: GlFormat) {
    let internal = format.internal;
    unsafe {
        gl::BindTexture(gl::TEXTURE_2D, pointer);
        gl::TexStorage2D(gl::TEXTURE_2D, 0, internal, width, height);
    }
}

/// Uploads a 3D texture to the GPU using `glTexImage3D`.
///
/// After upload the texture is not unbound, allowing the caller to set
/// parameters using `glTexParameterX` right after this call without having
/// to re-bind the texture.
fn upload_bytes_3d(
    pointer: u32,
    width: i32,
    height: i32,
    depth: i32,
    data: &[u8],
    format: GlFormat,
) {
    let internal = format.internal;
    let data_type = format.data_type;
    let format = format.format;

    unsafe {
        gl::BindTexture(gl::TEXTURE_3D, pointer);
        gl::TexImage3D(
            gl::TEXTURE_3D,
            0,
            internal as i32,
            width,
            height,
            depth,
            0,
            format,
            data_type,
            data.as_ptr().cast(),
        );
    }
}

fn sub_upload_bytes_3d(
    pointer: u32,
    x: i32,
    y: i32,
    z: i32,
    w: i32,
    h: i32,
    d: i32,
    data: &[u8],
    format: u32,
    data_type: u32,
) {
    unsafe {
        gl::BindTexture(gl::TEXTURE_3D, pointer);
        gl::TexSubImage3D(
            gl::TEXTURE_3D,
            0,
            x,
            y,
            z,
            w,
            h,
            d,
            format,
            data_type,
            data.as_ptr().cast(),
        );
    }
}

/// Allocates a 3D texture to the GPU using `glTexStorage3D`.
///
/// After allocation the texture is not unbound, allowing the caller to set
/// parameters using `glTexParameterX` right after this call without having
/// to re-bind the texture.
fn alloc_3d(pointer: u32, width: i32, height: i32, depth: i32, format: GlFormat) {
    let internal = format.internal;
    unsafe {
        gl::BindTexture(gl::TEXTURE_3D, pointer);
        gl::TexStorage3D(gl::TEXTURE_3D, 0, internal, width, height, depth);
    }
}

/// Uploads an array texture to the GPU using `glTexImage3D`.
///
/// After upload the texture is not unbound, allowing the caller to set
/// parameters using `glTexParameterX` right after this call without having
/// to re-bind the texture.
fn upload_bytes_array(
    pointer: u32,
    width: i32,
    height: i32,
    layers: i32,
    data: &[u8],
    format: GlFormat,
) {
    let internal = format.internal;
    let data_type = format.data_type;
    let format = format.format;

    unsafe {
        gl::BindTexture(gl::TEXTURE_2D_ARRAY, pointer);
        gl::TexImage3D(
            gl::TEXTURE_2D_ARRAY,
            0,
            internal as i32,
            width,
            height,
            layers,
            0,
            format,
            data_type,
            data.as_ptr().cast(),
        );
    }
}

fn sub_upload_bytes_array(
    pointer: u32,
    x: i32,
    y: i32,
    z: i32,
    w: i32,
    h: i32,
    d: i32,
    data: &[u8],
    format: u32,
    data_type: u32,
) {
    unsafe {
        gl::BindTexture(gl::TEXTURE_2D_ARRAY, pointer);
        gl::TexSubImage3D(
            gl::TEXTURE_2D_ARRAY,
            0,
            x,
            y,
            z,
            w,
            h,
            d,
            format,
            data_type,
            data.as_ptr().cast(),
        );
    }
}

/// Allocates an array texture to the GPU using `glTexStorage3D`.
///
/// After allocation the texture is not unbound, allowing the caller to set
/// parameters using `glTexParameterX` right after this call without having
/// to re-bind the texture.
fn alloc_array(pointer: u32, width: i32, height: i32, layers: i32, format: GlFormat) {
    let internal = format.internal;
    unsafe {
        gl::BindTexture(gl::TEXTURE_2D_ARRAY, pointer);
        gl::TexStorage3D(gl::TEXTURE_2D_ARRAY, 0, internal, width, height, layers);
    }
}

pub fn bind(target: TextureTarget, id: u32) {
    unsafe {
        gl::BindTexture(target.property_enum(), id);
    }
}

/// Sets min and mag filtering to the given `filtering`.
///
/// The mag filter is converted to the filtering "base type" (either nearest
/// or linear) with [`TextureFiltering::force_base_filtering`].
///
/// These correspond to the `GL_TEXTURE_MIN_FILTER` and `GL_TEXTURE_MAG_FILTER`
/// C OpenGL enums to set texture parameters.
pub fn set_filter(target: TextureTarget, filtering: TextureFiltering) {
    let target = target.property_enum();
    let mag_filtering = filtering.force_base_filtering().property_enum();
    let min_filtering = filtering.property_enum();

    unsafe {
        gl::TexParameteri(target, gl::TEXTURE_MIN_FILTER, min_filtering as i32);
        gl::TexParameteri(target, gl::TEXTURE_MAG_FILTER, mag_filtering as i32);
    }
}

/// Set ST texture wrapping for 2D textures.
///
/// These correspond to the `GL_TEXTURE_WRAP_S` and `GL_TEXTURE_WRAP_T` C
/// OpenGL enums to set texture parameters.
pub fn set_wrapping_st(target: TextureTarget, wrapping: TextureWrapping) {
    let target = target.property_enum();
    let wrapping = wrapping.property_enum();

    unsafe {
        gl::TexParameteri(target, gl::TEXTURE_WRAP_S, wrapping as i32);
        gl::TexParameteri(target, gl::TEXTURE_WRAP_T, wrapping as i32);
    }
}

/// Set R texture wrapping used in 3D textures.  
///
/// It is meant to be used in combination with [`set_wrapping_st`].
///
/// This corresponds to the `GL_TEXTURE_WRAP_R` C OpenGL enum to set the
/// texture parameter.
pub fn set_wrapping_r(target: TextureTarget, wrapping: TextureWrapping) {
    let wrapping = wrapping.property_enum();
    unsafe {
        gl::TexParameteri(target.property_enum(), gl::TEXTURE_WRAP_R, wrapping as i32);
    }
}
