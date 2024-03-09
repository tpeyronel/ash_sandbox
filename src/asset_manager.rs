use std::{
        ffi::OsString,
        hash::Hash,
        io::BufReader,
        marker::PhantomData,
        ops::{Index, IndexMut},
        path::{Path, PathBuf},
        process::Command,
        u32,
};

use bitflags::bitflags;
use crossbeam_channel::Receiver;
use ddsfile::DxgiFormat;
use gltf::accessor::{DataType, Dimensions};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use slotmap::{Key, SecondaryMap, SlotMap};
use thiserror::Error;

use crate::{
        application::ShaderSettings,
        components::Transform,
        constants::{
                DEFAULT_AMBIENT_STRENGTH, DEFAULT_DIFFUSE_STRENGTH, DEFAULT_MAG_FILTER, DEFAULT_MIN_FILTER,
                DEFAULT_SHININESS, DEFAULT_SPECULAR_STRENGTH,
        },
        hashmap::HashMap,
        my_glm::*,
        renderer::PrefilterParams,
        shader_preprocessor::{PreprocessedShaderStage, ShaderPreprocessor},
        shader_resource::{ShaderResource, ShaderResourceId, ShaderResourceProvider, ShaderResourceType},
        shader_resource_registry::ShaderResourceRegistry,
        shader_resources::{
                SHADER_RESOURCE_BILLBOARD_DATA, SHADER_RESOURCE_CUBE_SHADOW_MAP, SHADER_RESOURCE_ENVIRONMENT_MAP,
                SHADER_RESOURCE_EQUIRECTANGULAR_MAP, SHADER_RESOURCE_INPUT_FRAMEBUFFER, SHADER_RESOURCE_IRRADIANCE_MAP,
                SHADER_RESOURCE_MATERIAL_BASE_COLOR_TEXTURE, SHADER_RESOURCE_MATERIAL_DATA,
                SHADER_RESOURCE_MATERIAL_DIFFUSE_TEXTURE, SHADER_RESOURCE_MATERIAL_METALLIC_ROUGHNESS_TEXTURE,
                SHADER_RESOURCE_MATERIAL_NORMAL_TEXTURE, SHADER_RESOURCE_MATERIAL_SPECULAR_TEXTURE,
                SHADER_RESOURCE_OBJECT_MATRICES, SHADER_RESOURCE_PREFILTER_PARAMS, SHADER_RESOURCE_SHADER_SETTINGS,
                SHADER_RESOURCE_SHADOW_MAP, SHADER_RESOURCE_SKYBOX, SHADER_RESOURCE_WORLD_LIGHTS,
                SHADER_RESOURCE_WORLD_MATRICES,
        },
        util::{self, default, RefIntoBytesSlice},
        vk::vk_renderer::{BillboardData, MaterialData, ObjectMatrices, WorldLights, WorldMatrices},
        AnyResult,
};

/*enum ComponentType {
        I8 = 1,
        U8,
        I16,
        U16,
        U32,
        F32,
}

enum DataType {
        Scalar = 1,
        Vec2,
        Vec3,
        Vec4,
        Mat2,
        Mat3,
        Mat4,
}*/

slotmap::new_key_type! { pub struct ModelId; }

#[derive(Debug, Clone)]
pub struct Model {
        pub name: Option<String>,
        pub base_transform: Transform,
        pub meshes: Vec<MaterialMesh>,
        pub children: Vec<ModelId>,
}

pub struct MeshGroup(Vec<MaterialMesh>);

slotmap::new_key_type! { pub struct MeshId; }

#[derive(Debug)]
pub struct Mesh {
        pub positions: Vec<Vec3>,
        pub tex_coords: Vec<Vec2>,
        pub normals: Vec<Vec3>,
        pub tangents: Vec<Vec4>,
        pub indices: IndicesVec,
        pub bounding_box: BoundingBox,
}

#[derive(Debug, Clone)]
pub enum IndicesVec {
        U16(Vec<u16>),
        U32(Vec<u32>),
}

slotmap::new_key_type! { pub struct MaterialId; }

#[derive(Debug, Clone)]
pub struct Material {
        pub name: Option<String>,

        pub shader: ShaderId,

        pub base_color_factor: Vec4,
        pub metallic_factor: f32,
        pub roughness_factor: f32,
        pub shininess: f32,
        pub ambient_strength: f32,
        pub specular_strength: f32,
        pub diffuse_strength: f32,

        pub base_color_texture: TextureId,
        pub metallic_roughness_texture: TextureId,
        pub normal_texture: TextureId,
        pub occlusion_texture: Option<TextureId>,
        pub emissive_texture: Option<TextureId>,
        pub emissive_factor: Vec3,
}

impl Material {
        pub fn get_shader_resource_data<T>(
                &self,
                resource: &ShaderResourceId,
                f: impl FnOnce(Option<ShaderResourceData>) -> T,
        ) -> T {
                match resource {
                        r if *r == *SHADER_RESOURCE_MATERIAL_DIFFUSE_TEXTURE
                                || *r == *SHADER_RESOURCE_MATERIAL_BASE_COLOR_TEXTURE =>
                        {
                                f(Some(ShaderResourceData::Image2D(self.base_color_texture)))
                        },
                        r if *r == *SHADER_RESOURCE_MATERIAL_SPECULAR_TEXTURE
                                || *r == *SHADER_RESOURCE_MATERIAL_METALLIC_ROUGHNESS_TEXTURE =>
                        {
                                f(Some(ShaderResourceData::Image2D(self.metallic_roughness_texture)))
                        },
                        r if *r == *SHADER_RESOURCE_MATERIAL_NORMAL_TEXTURE => {
                                f(Some(ShaderResourceData::Image2D(self.normal_texture)))
                        },
                        r if *r == *SHADER_RESOURCE_MATERIAL_DATA => {
                                let data = MaterialData {
                                        ambient_color: self.base_color_factor,
                                        diffuse_color: self.base_color_factor,
                                        specular_color: self.base_color_factor,
                                        shininess_and_ambient_strength: Vec2::new(
                                                self.shininess,
                                                self.ambient_strength,
                                        ),
                                        specular_strength_and_diffuse_strength: Vec2::new(
                                                self.specular_strength,
                                                self.diffuse_strength,
                                        ),
                                };

                                f(Some(ShaderResourceData::StructData(unsafe { data.as_bytes() })))
                        },
                        _ => f(None),
                }
        }

        // pub fn get_shader_resource_data_or_default<T>(
        //         &self,
        //         resource: &ShaderResourceId,
        //         default: &Material,
        //         f: impl FnOnce(Option<ShaderResourceData>) -> T,
        // ) -> T {
        //         self.get_shader_resource_data(resource, |data| match data {
        //                 Some(data) => f(Some(data)),
        //                 None => default.get_shader_resource_data(resource, f),
        //         })
        // }
}

#[derive(Debug)]
pub enum ShaderResourceData<'a> {
        StructData(&'a [u8]),
        Image2D(TextureId),
}

#[derive(Debug, Clone, Copy)]
pub struct MaterialMesh {
        pub material: MaterialId,
        pub mesh: MeshId,
}

slotmap::new_key_type! { pub struct TextureId; }

#[derive(Debug, Clone)]
pub struct Texture {
        pub name: Option<String>,
        pub image: ImageId,
        pub sampler: SamplerId,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
        R8,
        R8G8B8,
        R8G8B8A8,
        R16G16B16A16, // f16
        R32G32B32,    // f32
        R32G32B32A32, // f32
        BC1_UNORM,    // rgba
        BC2_UNORM,    // rgba
        BC3_UNORM,    // rgba
        BC4_UNORM,    // r
        BC4_SNORM,    // r
        BC5_UNORM,    // rg
        BC5_SNORM,    // rg
        BC6H_UFLOAT,  // rgb hdr
        BC6H_SFLOAT,  // rgb hdr
        BC7_UNORM,    // rgb/rgba
}

impl ImageFormat {
        fn from_gltf_format(format: gltf::image::Format) -> Option<Self> {
                Some(match format {
                        gltf::image::Format::R8 => Self::R8,
                        gltf::image::Format::R8G8B8 => Self::R8G8B8,
                        gltf::image::Format::R8G8B8A8 => Self::R8G8B8A8,
                        _ => return None,
                })
        }

        fn compute_stride(&self, width: u32, height: u32) -> u32 {
                util::compute_image_stride(width, height, self.block_size(), self.block_extent())
        }

        fn block_extent(&self) -> (u32, u32) {
                match self {
                        ImageFormat::R8
                        | ImageFormat::R8G8B8
                        | ImageFormat::R8G8B8A8
                        | ImageFormat::R16G16B16A16
                        | ImageFormat::R32G32B32
                        | ImageFormat::R32G32B32A32 => (1, 1),

                        ImageFormat::BC1_UNORM
                        | ImageFormat::BC2_UNORM
                        | ImageFormat::BC3_UNORM
                        | ImageFormat::BC4_UNORM
                        | ImageFormat::BC4_SNORM
                        | ImageFormat::BC5_UNORM
                        | ImageFormat::BC5_SNORM
                        | ImageFormat::BC6H_UFLOAT
                        | ImageFormat::BC6H_SFLOAT
                        | ImageFormat::BC7_UNORM => (4, 4),
                }
        }

        fn block_size(&self) -> u32 {
                match self {
                        ImageFormat::R8 => 1,
                        ImageFormat::R8G8B8 => 3,
                        ImageFormat::R8G8B8A8 => 4,
                        ImageFormat::R16G16B16A16 => 8,
                        ImageFormat::R32G32B32 => 12,
                        ImageFormat::R32G32B32A32 => 16,
                        ImageFormat::BC1_UNORM => 8,
                        ImageFormat::BC2_UNORM => 16,
                        ImageFormat::BC3_UNORM => 16,
                        ImageFormat::BC4_UNORM => 8,
                        ImageFormat::BC4_SNORM => 8,
                        ImageFormat::BC5_UNORM => 16,
                        ImageFormat::BC5_SNORM => 16,
                        ImageFormat::BC6H_UFLOAT => 16,
                        ImageFormat::BC6H_SFLOAT => 16,
                        ImageFormat::BC7_UNORM => 16,
                }
        }
}

slotmap::new_key_type! { pub struct ImageId; }

#[derive(Debug, Clone, Error)]
pub enum LoadImageError {
        #[error("dxgi format {0:?} is not supported")]
        UnsupportedDxgiFormat(DxgiFormat),
        #[error("d3d format {0:?} is not supported")]
        UnsupportedD3DFormat(ddsfile::D3DFormat),
        #[error("pixel format {0:?} is not supported")]
        UnsupportedPixelFormat(ddsfile::PixelFormat),
}

bitflags! {
        pub struct ImageFlags: u32 {
                const CUBEMAP = 1 << 0;
        }
}

#[derive(Debug, Clone)]
pub struct Image {
        pub name: Option<String>,
        pub flags: ImageFlags,
        data: Vec<u8>, // pixel data as raw bytes. `format` must be used to correctly interpret these values.
        pub width: u32,
        pub height: u32,
        pub mipmaps: u32, // amount of mip levels. Must be at least 1
        pub layers: u32,  // amount of array layers
        pub format: ImageFormat,
        pub color_space: ColorSpace,

        mipmap_strides: Vec<u32>, // how many bytes each mipmap takes.
        layer_stride: u32,        // how many bytes a layer with all mipmaps takes.
}

impl Image {
        #[allow(dead_code)]
        pub fn from_rgb(name: Option<String>, rgb: [u8; 3], width: u32, height: u32, color_space: ColorSpace) -> Self {
                Self::from_data(name, rgb.to_vec(), width, height, ImageFormat::R8G8B8, color_space)
        }

        #[allow(dead_code)]
        pub fn from_rgb_1x1(name: Option<String>, rgb: [u8; 3], color_space: ColorSpace) -> Self {
                Self::from_rgb(name, rgb, 1, 1, color_space)
        }

        #[allow(dead_code)]
        pub fn from_rgba(
                name: Option<String>,
                rgba: [u8; 4],
                width: u32,
                height: u32,
                color_space: ColorSpace,
        ) -> Self {
                Self::from_data(name, rgba.to_vec(), width, height, ImageFormat::R8G8B8A8, color_space)
        }

        #[allow(dead_code)]
        pub fn from_rgba_1x1(name: Option<String>, rgba: [u8; 4], color_space: ColorSpace) -> Self {
                Self::from_rgba(name, rgba, 1, 1, color_space)
        }

        pub fn from_data(
                name: Option<String>,
                data: Vec<u8>,
                width: u32,
                height: u32,
                format: ImageFormat,
                color_space: ColorSpace,
        ) -> Self {
                let mipmaps = 1;
                let layers = 1;

                let stride = format.compute_stride(width, height);
                assert_eq!(data.len() as u32, stride);

                let mipmap_strides = vec![stride];
                let layer_stride = stride;

                Image {
                        name,
                        data,
                        flags: ImageFlags::empty(),
                        width,
                        height,
                        mipmaps,
                        layers,
                        format,
                        color_space,
                        mipmap_strides,
                        layer_stride,
                }
        }

        pub fn from_file(path: &Path, color_space: ColorSpace) -> AnyResult<Self> {
                let image_name = path.to_string_lossy().to_string();

                if path.extension().unwrap().eq_ignore_ascii_case("dds") {
                        // FIXME: we ignore color_space in this case.
                        return Self::from_dds_file(path, image_name);
                }

                let image = image::open(path)?;

                let format = match image {
                        image::DynamicImage::ImageRgb8(_) => ImageFormat::R8G8B8,
                        image::DynamicImage::ImageRgba8(_) => ImageFormat::R8G8B8A8,
                        image::DynamicImage::ImageRgb32F(_) => ImageFormat::R32G32B32,
                        image::DynamicImage::ImageRgba32F(_) => ImageFormat::R32G32B32A32,
                        _ => panic!("Unsupported image format! {:?}", image),
                };

                let width = image.width();
                let height = image.height();
                let data = image.into_bytes();
                let mipmaps = 1;
                let layers = 1;
                let mipmap_strides = vec![data.len() as u32];
                let layer_stride = data.len() as u32;

                Ok(Self {
                        name: Some(image_name),
                        data,
                        flags: ImageFlags::empty(),
                        width,
                        height,
                        mipmaps,
                        layers,
                        format,
                        color_space,
                        mipmap_strides,
                        layer_stride,
                })
        }

        pub fn from_files(
                name: Option<String>,
                paths: &[&Path],
                flags: ImageFlags,
                color_space: ColorSpace,
        ) -> AnyResult<Self> {
                assert!(!paths.is_empty());

                let images = paths
                        .into_iter()
                        .map(|p| Self::from_file(p, color_space))
                        .collect::<AnyResult<Vec<Self>>>()?;

                for image in &images {
                        assert_eq!(1, image.layers);
                }

                let data_len = images[0].data.len();
                let width = images[0].width;
                let height = images[0].height;
                let format = images[0].format;
                let mipmaps = images[0].mipmaps;
                let layer_stride = images[0].layer_stride;
                let mipmap_strides = &images[0].mipmap_strides;

                for image in &images[1..] {
                        assert_eq!(width, image.width);
                        assert_eq!(height, image.height);
                        assert_eq!(format, image.format);
                        assert_eq!(mipmaps, image.mipmaps);
                        assert_eq!(layer_stride, image.layer_stride);
                        assert_eq!(mipmap_strides, &image.mipmap_strides);
                }

                for image in &images {
                        assert_eq!(layer_stride as usize, image.data.len());
                }

                let mut data = Vec::with_capacity(data_len * images.len());
                for image in &images {
                        data.extend_from_slice(&image.data);
                }

                let layers = images.len() as u32;

                let merged = Self {
                        name,
                        data,
                        flags,
                        width,
                        height,
                        mipmaps,
                        layers,
                        format,
                        color_space,
                        mipmap_strides: mipmap_strides.clone(),
                        layer_stride,
                };

                Ok(merged)
        }

        pub fn from_files_cubemap(name: Option<String>, paths: [&Path; 6], color_space: ColorSpace) -> AnyResult<Self> {
                Self::from_files(name, &paths, ImageFlags::CUBEMAP, color_space)
        }

        pub fn get_data(&self, layer: u32, mipmap: u32) -> &[u8] {
                let layer_offset = layer as usize * self.layer_stride as usize;
                let mipmap_offset = self.mipmap_strides[0..mipmap as usize].iter().sum::<u32>() as usize;
                let offset = layer_offset + mipmap_offset;
                let mipmap_stride = self.mipmap_strides[mipmap as usize] as usize;

                &self.data[offset..offset + mipmap_stride]
        }

        fn from_dds_file(path: &Path, image_name: String) -> AnyResult<Self> {
                let file = std::fs::File::open(path)?;
                let reader = BufReader::new(file);
                let dds = ddsfile::Dds::read(reader)?;

                let width = dds.get_width();
                let height = dds.get_height();
                let mipmaps = dds.get_num_mipmap_levels();
                let layers = if let Some(h10) = &dds.header10 {
                        if dds.header.caps2.contains(ddsfile::Caps2::CUBEMAP) {
                                h10.array_size * 6
                        } else {
                                h10.array_size
                        }
                } else {
                        1
                };
                let (format, color_space) = Self::try_format_and_color_space_from_dds(&dds)?;

                let mipmap_strides = {
                        let format = dds.get_format().expect("couldn't extract format from dds");
                        let mut curr_width = dds.get_width();
                        let mut curr_height = dds.get_height();

                        let mut mipmap_strides = vec![];
                        for _ in 0..mipmaps {
                                let stride = compute_dds_mipmap_stride(curr_width, curr_height, &format);
                                mipmap_strides.push(stride);
                                curr_width = 1.max(curr_width / 2);
                                curr_height = 1.max(curr_height / 2);
                        }
                        mipmap_strides
                };

                let layer_stride = dds.get_array_stride().expect("unsupported format");

                let mut flags = ImageFlags::empty();
                if let Some(h10) = dds.header10 {
                        if h10.misc_flag.contains(ddsfile::MiscFlag::TEXTURECUBE) {
                                flags.insert(ImageFlags::CUBEMAP)
                        }
                }

                let data = dds.data;

                Ok(Self {
                        name: Some(image_name),
                        data,
                        flags,
                        width,
                        height,
                        mipmaps,
                        layers,
                        format,
                        color_space,
                        mipmap_strides,
                        layer_stride,
                })
        }

        fn try_format_and_color_space_from_dds(
                dds: &ddsfile::Dds,
        ) -> Result<(ImageFormat, ColorSpace), LoadImageError> {
                let format_color_space = if let Some(dxgi_format) = dds.get_dxgi_format() {
                        match dxgi_format {
                                DxgiFormat::R32G32B32A32_Float => (ImageFormat::R32G32B32A32, ColorSpace::Linear),
                                DxgiFormat::R16G16B16A16_Float => (ImageFormat::R16G16B16A16, ColorSpace::Linear),
                                DxgiFormat::R8G8B8A8_UNorm => (ImageFormat::R8G8B8A8, ColorSpace::Linear),
                                DxgiFormat::R8G8B8A8_UNorm_sRGB => (ImageFormat::R8G8B8A8, ColorSpace::Srgb),
                                DxgiFormat::R8_UNorm => (ImageFormat::R8, ColorSpace::Linear),
                                DxgiFormat::BC1_UNorm => (ImageFormat::BC1_UNORM, ColorSpace::Linear),
                                DxgiFormat::BC1_UNorm_sRGB => (ImageFormat::BC1_UNORM, ColorSpace::Srgb),
                                DxgiFormat::BC2_UNorm => (ImageFormat::BC2_UNORM, ColorSpace::Linear),
                                DxgiFormat::BC2_UNorm_sRGB => (ImageFormat::BC2_UNORM, ColorSpace::Srgb),
                                DxgiFormat::BC3_UNorm => (ImageFormat::BC3_UNORM, ColorSpace::Linear),
                                DxgiFormat::BC3_UNorm_sRGB => (ImageFormat::BC3_UNORM, ColorSpace::Srgb),
                                DxgiFormat::BC4_UNorm => (ImageFormat::BC4_UNORM, ColorSpace::Linear),
                                DxgiFormat::BC4_SNorm => (ImageFormat::BC4_SNORM, ColorSpace::Linear),
                                DxgiFormat::BC5_UNorm => (ImageFormat::BC5_UNORM, ColorSpace::Linear),
                                DxgiFormat::BC5_SNorm => (ImageFormat::BC5_SNORM, ColorSpace::Linear),
                                DxgiFormat::BC6H_UF16 => (ImageFormat::BC6H_UFLOAT, ColorSpace::Linear),
                                DxgiFormat::BC6H_SF16 => (ImageFormat::BC6H_SFLOAT, ColorSpace::Linear),
                                DxgiFormat::BC7_UNorm => (ImageFormat::BC7_UNORM, ColorSpace::Linear),
                                DxgiFormat::BC7_UNorm_sRGB => (ImageFormat::BC7_UNORM, ColorSpace::Srgb),
                                f => return Err(LoadImageError::UnsupportedDxgiFormat(f)),
                        }
                } else if let Some(d3d_format) = dds.get_d3d_format() {
                        return Err(LoadImageError::UnsupportedD3DFormat(d3d_format));
                } else {
                        return Err(LoadImageError::UnsupportedPixelFormat(dds.header.spf.clone()));
                };

                Ok(format_color_space)
        }
}

pub type MagFilter = gltf::texture::MagFilter;
pub type MinFilter = gltf::texture::MinFilter;
pub type WrappingMode = gltf::texture::WrappingMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
        Srgb,
        Linear,
}

bitflags! {
        struct ImageUsage: u32 {
                const BASE_COLOR = 1 << 0;
                const METALLIC_ROUGHNESS = 1 << 1;
                const NORMAL = 1 << 2;
                const OCCLUSION = 1 << 3;
                const EMISSIVE = 1 << 4;
        }
}

slotmap::new_key_type! { pub struct SamplerId; }

#[derive(Debug, Clone)]
pub struct Sampler {
        pub name: Option<String>,
        pub mag_filter: MagFilter,
        pub min_filter: MinFilter,
        pub wrap_s: WrappingMode,
        pub wrap_t: WrappingMode,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum CullMode {
        #[serde(rename = "none")]
        None,
        #[serde(rename = "front")]
        Front,
        #[serde(rename = "back")]
        Back,
}

impl Default for CullMode {
        fn default() -> Self {
                Self::Back
        }
}

#[derive(Debug, serde::Deserialize)]
pub struct ShaderDeclaration {
        pub name: String,

        #[serde(rename = "vertex-shader")]
        pub vert_shader: PathBuf,

        #[serde(rename = "fragment-shader")]
        pub frag_shader: Option<PathBuf>,

        // default is false
        #[serde(rename = "disable-depth-test", default)]
        pub disable_depth_test: bool,

        #[serde(rename = "cull-mode", default)]
        pub cull_mode: CullMode,

        pub uniforms: Vec<String>,

        #[serde(rename = "vertex-inputs")]
        pub vertex_inputs: Vec<String>,

        #[serde(rename = "render-stage", default)]
        pub render_stage: ShaderRenderStage,

        #[serde(rename = "push-constants-size", default)]
        pub push_constants_size: u32,
}

slotmap::new_key_type! { pub struct ShaderId; }

#[derive(Debug, Clone)]
pub struct Shader {
        pub name: String,
        pub vert_shader: PreprocessedShaderStage,
        pub frag_shader: Option<PreprocessedShaderStage>,
        pub disable_depth_test: bool,
        pub cull_mode: CullMode,
        pub vertex_inputs: Vec<String>,
        pub render_stage: ShaderRenderStage,
        pub push_constants_size: u32,
}

impl Shader {
        pub fn from_yaml(shader_resources: &ShaderResourceRegistry, path: &Path) -> Result<Self, ShaderLoadError> {
                let yaml = std::fs::read_to_string(path)?;
                let declaration: ShaderDeclaration = serde_yaml::from_str(&yaml)?;

                let directory = path
                        .parent()
                        .ok_or_else(|| ShaderLoadError::InvalidPath(format!("Path {:?} does not have parent", path)))?;

                Ok(Self {
                        name: declaration.name.clone(),
                        vert_shader: ShaderPreprocessor::preprocess_glsl_source(
                                shader_resources,
                                directory.join(&declaration.vert_shader),
                        )?,
                        frag_shader: declaration
                                .frag_shader
                                .as_ref()
                                .map(|p| {
                                        ShaderPreprocessor::preprocess_glsl_source(shader_resources, directory.join(p))
                                })
                                .transpose()?,
                        disable_depth_test: declaration.disable_depth_test,
                        cull_mode: declaration.cull_mode,
                        vertex_inputs: declaration.vertex_inputs,
                        render_stage: declaration.render_stage,
                        push_constants_size: declaration.push_constants_size,
                })
        }
}

#[derive(Debug)]
pub struct ShaderModule {
        pub bin: Vec<u8>,
}

impl ShaderModule {
        pub fn from_glsl_file(path: PathBuf) -> Result<Self, ShaderLoadError> {
                let input_path = path.into_os_string().into_string()?;
                let output_path = format!("{}.spv", input_path);

                let mut child = Command::new("res/misc/glslangValidator.exe")
                        .arg(input_path)
                        .arg("--target-env")
                        .arg("vulkan1.2")
                        .arg("-o")
                        .arg(&output_path)
                        .spawn()?;

                let exit_status = child.wait()?;

                if !exit_status.success() {
                        return Err(ShaderLoadError::CompileError(exit_status));
                }

                let bin = std::fs::read(&output_path)?;

                Ok(Self { bin })
        }
}

#[derive(Enum, Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShaderRenderStage {
        SkyboxMapping,
        PointShadowMapping,
        DirectionalShadowMapping,
        Drawing,
        Postprocessing,
}

impl Default for ShaderRenderStage {
        fn default() -> Self {
                Self::Drawing
        }
}

#[derive(Debug, Clone)]
pub struct BoundingBox {
        pub min: Vec3,
        pub max: Vec3,
}

impl From<&gltf::mesh::BoundingBox> for BoundingBox {
        fn from(bbox: &gltf::mesh::BoundingBox) -> Self {
                Self {
                        min: bbox.min.into(),
                        max: bbox.max.into(),
                }
        }
}

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum GLTFImportError {
        #[error("image source is not a uri")]
        ImageSourceNotUri,
        #[error("image source uri is not relative")]
        ImageSourceUriNotRelative,
        #[error("image format not supported {0:?}")]
        ImageFormatNotSupported(gltf::image::Format),
        #[error("accessor missing buffer view")]
        AccessorMissingBufferView,
        #[error("mesh missing primitives")]
        MeshMissingPrimitives,
        #[error("mesh missing positions")]
        MeshMissingPositions,
        #[error("mesh missing tex coords")]
        MeshMissingTexCoords,
        #[error("mesh missing normals")]
        MeshMissingNormals,
        #[error("mesh missing tangents")]
        MeshMissingTangents,
        #[error("mesh missing indices")]
        MeshMissingIndices,
        #[error("model uses a matrix that is not decomposed")]
        RootModelMissing,
        #[error("scene name missing")]
        SceneNameMissing,
        #[error("scene name alredy registered")]
        SceneNameAlreadyRegistered,
        #[error(transparent)]
        GLTFCrateError(#[from] gltf::Error),
}

#[derive(Error, Debug)]
pub enum ShaderLoadError {
        #[error("invalid path: {0}")]
        InvalidPath(String),
        #[error(transparent)]
        YamlError(#[from] serde_yaml::Error),
        #[error("path has invalid unicode: {}", PathBuf::from(.0).display())]
        InvalidUnicode(OsString),
        #[error("shader already registered: {0}")]
        ShaderNameAlreadyRegistered(String),
        #[error(transparent)]
        IoError(#[from] std::io::Error),
        #[error("error ocurred compiling shaders: {0}")]
        CompileError(std::process::ExitStatus),
        #[error("invalid preprocessor directive syntax for '{0}'")]
        InvalidPreprocessorDirective(&'static str),
        #[error("unknown shader resource id '{0}'")]
        UnknownShaderResourceId(String),
        #[error("mismatched shader resource type: '{0}' vs '{1}'")]
        MismatchedShaderResourceType(String, String),
}

impl From<OsString> for ShaderLoadError {
        fn from(s: OsString) -> Self {
                Self::InvalidUnicode(s)
        }
}

slotmap::new_key_type! { pub struct CubemapId; }

#[derive(Debug, Clone)]
pub enum Cubemap {
        Faces(Image),           // image must be a cubemap
        Equirectangular(Image), // image must be a equirectangular image
}

/* pub struct AssetManagerBuilder {
        gltf_paths: Vec<PathBuf>,
}

impl AssetManagerBuilder {
        pub fn new() -> Self {
                Self { gltf_paths: Vec::new() }
        }
} */

pub struct AssetBundle {
        pub assets: AssetStorage,
}

impl AssetBundle {
        pub fn from_gltf(gltf: &Path) -> Result<(Self, String), GLTFImportError> {
                scoped_timer!("Loaded gltf in: ", Millis);

                let (mut assets, _event_rx) = AssetStorage::new();

                let (doc, buffer_data, image_data) = gltf::import(gltf)?;

                let image_usages_by_index = Self::discover_image_usages(&doc, &image_data);
                let images_by_index =
                        Self::load_images(gltf, &doc, image_data, &image_usages_by_index, &mut assets.images)?;
                let samplers_by_index = Self::load_samplers(&doc, &mut assets.samplers);
                let textures_by_index =
                        Self::load_textures(&doc, &images_by_index, &samplers_by_index, &mut assets.textures);
                let materials_by_index = Self::load_materials(&doc, &textures_by_index, &mut assets.materials);
                let mesh_groups_by_index =
                        Self::load_meshes(&doc, buffer_data, &materials_by_index, &mut assets.meshes)?;
                let models_by_index = Self::load_models(&doc, &mesh_groups_by_index, &mut assets.models)?;
                let root_model = Self::load_root_model(&doc, &models_by_index, &mut assets.models)?;
                assets.named_models.insert(root_model.0.clone(), root_model.1);

                Ok((Self { assets }, root_model.0))
        }

        fn discover_image_usages(doc: &gltf::Document, image_data: &Vec<gltf::image::Data>) -> Vec<ImageUsage> {
                let mut usages = vec![ImageUsage::empty(); image_data.len()];

                let mut process_texture = |t: Option<gltf::texture::Texture<'_>>, usage: ImageUsage| {
                        if let Some(t) = t {
                                usages[t.index()].insert(usage);
                        }
                };

                for material in doc.materials() {
                        let pbr = material.pbr_metallic_roughness();

                        process_texture(pbr.base_color_texture().map(|t| t.texture()), ImageUsage::BASE_COLOR);
                        process_texture(
                                pbr.metallic_roughness_texture().map(|t| t.texture()),
                                ImageUsage::METALLIC_ROUGHNESS,
                        );
                        process_texture(material.normal_texture().map(|t| t.texture()), ImageUsage::NORMAL);
                        process_texture(material.occlusion_texture().map(|t| t.texture()), ImageUsage::OCCLUSION);
                        process_texture(material.emissive_texture().map(|t| t.texture()), ImageUsage::EMISSIVE);
                }

                usages
        }

        fn load_images(
                gltf_path: &Path,
                doc: &gltf::Document,
                image_data: Vec<gltf::image::Data>,
                image_usages: &Vec<ImageUsage>,
                out_images: &mut ObservableSlotMap<ImageId, Image, AssetManagerEvent>,
        ) -> Result<Vec<ImageId>, GLTFImportError> {
                // TODO: load images ourselves

                image_data
                        .into_iter()
                        .zip(image_usages)
                        .zip(doc.images())
                        .map(|((image, image_usage), json_image)| {
                                let Some(format) = ImageFormat::from_gltf_format(image.format) else {
                                        return Err(GLTFImportError::ImageFormatNotSupported(image.format));
                                };

                                // Image path relative to working directory
                                let relative_path = match json_image.source() {
                                        gltf::image::Source::Uri { uri, .. } => {
                                                if uri.contains(':') {
                                                        error!("Trying to import image with non relative uri!");
                                                        return Err(GLTFImportError::ImageSourceUriNotRelative);
                                                }

                                                gltf_path.join(uri)
                                        },
                                        _ => {
                                                error!("Image source is not an uri!");
                                                return Err(GLTFImportError::ImageSourceNotUri);
                                        },
                                }
                                .clean();

                                let color_space = Self::color_space_from_usage(*image_usage);

                                let width = image.width;
                                let height = image.height;
                                let data = image.pixels;
                                let mipmap_strides = vec![data.len() as u32];
                                let layer_stride = data.len() as u32;

                                let image_id = out_images.insert(Image {
                                        name: Some(relative_path.to_string_lossy().to_string()),
                                        data,
                                        flags: ImageFlags::empty(),
                                        width,
                                        height,
                                        mipmaps: 1,
                                        layers: 1,
                                        format,
                                        color_space,
                                        mipmap_strides,
                                        layer_stride,
                                });

                                Ok(image_id)
                        })
                        .collect()
        }

        fn color_space_from_usage(image_usage: ImageUsage) -> ColorSpace {
                if image_usage.intersects(
                        ImageUsage::METALLIC_ROUGHNESS
                                | ImageUsage::NORMAL
                                | ImageUsage::OCCLUSION
                                | ImageUsage::EMISSIVE,
                ) {
                        ColorSpace::Linear
                } else {
                        ColorSpace::Srgb
                }
        }

        fn load_samplers(
                doc: &gltf::Document,
                out_samplers: &mut ObservableSlotMap<SamplerId, Sampler, AssetManagerEvent>,
        ) -> Vec<SamplerId> {
                doc.samplers()
                        .map(|s| {
                                let sampler_id = out_samplers.insert(Sampler {
                                        name: s.name().map(String::from),
                                        mag_filter: s.mag_filter().unwrap_or(DEFAULT_MAG_FILTER),
                                        min_filter: s.min_filter().unwrap_or(DEFAULT_MIN_FILTER),
                                        wrap_s: s.wrap_s(),
                                        wrap_t: s.wrap_t(),
                                });

                                sampler_id
                        })
                        .collect()
        }

        fn load_textures(
                doc: &gltf::Document,
                images_by_index: &[ImageId],
                samplers_by_index: &[SamplerId],
                out_textures: &mut ObservableSlotMap<TextureId, Texture, AssetManagerEvent>,
        ) -> Vec<TextureId> {
                doc.textures()
                        .map(|t| {
                                let texture = Texture {
                                        name: t.name().map(String::from),
                                        image: images_by_index[t.source().index()],
                                        sampler: t
                                                .sampler()
                                                .index()
                                                .map_or(SamplerId::default(), |i| samplers_by_index[i]),
                                };

                                let tex_id = out_textures.insert(texture);

                                tex_id
                        })
                        .collect()
        }

        fn load_materials(
                doc: &gltf::Document,
                textures_by_index: &[TextureId],
                out_materials: &mut ObservableSlotMap<MaterialId, Material, AssetManagerEvent>,
        ) -> Vec<MaterialId> {
                doc.materials()
                        .map(|m| {
                                // TODO: handle textures better
                                let pbr_mr = m.pbr_metallic_roughness();

                                let base_color_factor = pbr_mr.base_color_factor().into();
                                let metallic_factor = pbr_mr.metallic_factor();
                                let roughness_factor = pbr_mr.roughness_factor();
                                let base_color_texture = pbr_mr
                                        .base_color_texture()
                                        .map_or(TextureId::default(), |t| textures_by_index[t.texture().index()]);
                                let metallic_roughness_texture = pbr_mr
                                        .metallic_roughness_texture()
                                        .map_or(TextureId::default(), |t| textures_by_index[t.texture().index()]);
                                let normal_texture = m
                                        .normal_texture()
                                        .map_or(TextureId::default(), |t| textures_by_index[t.texture().index()]);
                                let occlusion_texture =
                                        m.occlusion_texture().map(|t| textures_by_index[t.texture().index()]);
                                let emissive_texture =
                                        m.emissive_texture().map(|t| textures_by_index[t.texture().index()]);
                                let emissive_factor = Vec3::from_slice(&m.emissive_factor());

                                let material = Material {
                                        name: m.name().map(String::from),
                                        shader: ShaderId::default(),
                                        base_color_factor,
                                        metallic_factor,
                                        shininess: DEFAULT_SHININESS,
                                        ambient_strength: DEFAULT_AMBIENT_STRENGTH,
                                        specular_strength: DEFAULT_SPECULAR_STRENGTH,
                                        diffuse_strength: DEFAULT_DIFFUSE_STRENGTH,
                                        roughness_factor,
                                        base_color_texture,
                                        metallic_roughness_texture,
                                        normal_texture,
                                        occlusion_texture,
                                        emissive_texture,
                                        emissive_factor,
                                };

                                let mat_id = out_materials.insert(material);

                                mat_id
                        })
                        .collect()
        }

        fn load_meshes(
                doc: &gltf::Document,
                buffer_data: Vec<gltf::buffer::Data>,
                materials_by_index: &[MaterialId],
                out_meshes: &mut ObservableSlotMap<MeshId, Mesh, AssetManagerEvent>,
        ) -> Result<Vec<MeshGroup>, GLTFImportError> {
                let accessors: Vec<gltf::Accessor> = doc.accessors().collect();

                let mut mesh_groups = Vec::new();

                for m in doc.meshes() {
                        let mut mesh_group = MeshGroup(Vec::new());

                        for p in m.primitives() {
                                let positions = match p.get(&gltf::Semantic::Positions) {
                                        Some(positions) => {
                                                read_gltf_accessor(&buffer_data, &accessors[positions.index()])
                                        },
                                        None => return Err(GLTFImportError::MeshMissingPositions),
                                };

                                let tex_coords = match p.get(&gltf::Semantic::TexCoords(0)) {
                                        Some(tex_coords) => {
                                                read_gltf_accessor(&buffer_data, &accessors[tex_coords.index()])
                                        },
                                        None => return Err(GLTFImportError::MeshMissingTexCoords),
                                };

                                let normals = match p.get(&gltf::Semantic::Normals) {
                                        Some(normals) => read_gltf_accessor(&buffer_data, &accessors[normals.index()]),
                                        None => return Err(GLTFImportError::MeshMissingNormals),
                                };

                                let tangents = match p.get(&gltf::Semantic::Tangents) {
                                        Some(tangents) => {
                                                read_gltf_accessor(&buffer_data, &accessors[tangents.index()])
                                        },
                                        None => return Err(GLTFImportError::MeshMissingTangents),
                                };

                                let indices = match p.indices() {
                                        Some(indices) => {
                                                let accessor = &accessors[indices.index()];

                                                match accessor.data_type() {
                                                        DataType::U16 => IndicesVec::U16(read_gltf_accessor(
                                                                &buffer_data,
                                                                accessor,
                                                        )),
                                                        DataType::U32 => IndicesVec::U32(read_gltf_accessor(
                                                                &buffer_data,
                                                                accessor,
                                                        )),
                                                        _ => panic!("Invalid indices data type"),
                                                }
                                        },
                                        None => return Err(GLTFImportError::MeshMissingIndices),
                                };

                                let material = p
                                        .material()
                                        .index()
                                        .map_or(MaterialId::default(), |i| materials_by_index[i]);

                                let mesh = Mesh {
                                        positions,
                                        tex_coords,
                                        normals,
                                        tangents,
                                        indices,
                                        bounding_box: BoundingBox::from(&p.bounding_box()),
                                };

                                let mesh_id = out_meshes.insert(mesh);

                                mesh_group.0.push(MaterialMesh {
                                        material,
                                        mesh: mesh_id,
                                });
                        }

                        mesh_groups.push(mesh_group);
                }

                Ok(mesh_groups)
        }

        fn load_models(
                doc: &gltf::Document,
                mesh_groups: &[MeshGroup],
                out_models: &mut ObservableSlotMap<ModelId, Model, AssetManagerEvent>,
        ) -> Result<Vec<ModelId>, GLTFImportError> {
                let mut model_ids = Vec::new();

                for n in doc.nodes() {
                        let meshes =
                                n.mesh().map(|mg| mesh_groups[mg.index()].0.clone())
                                        .unwrap_or_else(Vec::new);

                        let children = n.children().map(|n| model_ids[n.index()]).collect();

                        let base_transform = match &n.transform() {
                                gltf::scene::Transform::Matrix { matrix } => unsafe {
                                        let mat = Mat4::from_cols_slice(std::slice::from_raw_parts(
                                                matrix as *const _ as *const f32,
                                                16,
                                        ));

                                        Transform::from_mat4(&mat)
                                },
                                gltf::scene::Transform::Decomposed {
                                        translation,
                                        rotation,
                                        scale,
                                } => {
                                        let translation = Vec3::from_slice(translation);

                                        let rotation = Quat::from_slice(rotation).normalize();

                                        let scale = Vec3::from_slice(scale);

                                        Transform {
                                                translation,
                                                rotation,
                                                scale,
                                        }
                                },
                        };

                        let model_id = out_models.insert(Model {
                                name: n.name().map(String::from),
                                base_transform,
                                meshes,
                                children,
                        });

                        model_ids.push(model_id);
                }

                Ok(model_ids)
        }

        fn load_root_model(
                doc: &gltf::Document,
                models_by_index: &[ModelId],
                out_models: &mut ObservableSlotMap<ModelId, Model, AssetManagerEvent>,
        ) -> Result<(String, ModelId), GLTFImportError> {
                if doc.scenes().len() > 1 {
                        warn!("More than 1 root model in GLTF document");
                }

                let scene = match doc.scenes().next() {
                        Some(scene) => scene,
                        None => return Err(GLTFImportError::RootModelMissing),
                };

                let name = match scene.name() {
                        Some(name) => name.to_string(),
                        None => return Err(GLTFImportError::SceneNameMissing),
                };

                let children = scene.nodes().map(|n| models_by_index[n.index()]).collect();

                let model_id = out_models.insert(Model {
                        name: Some(name.clone()),
                        base_transform: Transform::from_rotation(Quat::from_axis_angle(
                                Vec3::UP,
                                180.0f32.to_radians(),
                        )),
                        meshes: Vec::new(),
                        children,
                });

                Ok((name, model_id))
        }
}

#[derive(Debug)]
pub struct AssetStorage {
        images: ObservableSlotMap<ImageId, Image, AssetManagerEvent>,
        samplers: ObservableSlotMap<SamplerId, Sampler, AssetManagerEvent>,
        textures: ObservableSlotMap<TextureId, Texture, AssetManagerEvent>,
        pub materials: ObservableSlotMap<MaterialId, Material, AssetManagerEvent>,
        meshes: ObservableSlotMap<MeshId, Mesh, AssetManagerEvent>,
        models: ObservableSlotMap<ModelId, Model, AssetManagerEvent>,
        shaders: ObservableSlotMap<ShaderId, Shader, AssetManagerEvent>,
        cubemaps: ObservableSlotMap<CubemapId, Cubemap, AssetManagerEvent>,

        named_models: HashMap<String, ModelId>,

        shader_resources: ShaderResourceRegistry,
}

impl AssetStorage {
        pub fn new() -> (Self, Receiver<AssetManagerEvent>) {
                let (event_tx, event_rx) = crossbeam_channel::unbounded();

                (
                        Self {
                                images: ObservableSlotMap::new(event_tx.clone()),
                                samplers: ObservableSlotMap::new(event_tx.clone()),
                                textures: ObservableSlotMap::new(event_tx.clone()),
                                materials: ObservableSlotMap::new(event_tx.clone()),
                                meshes: ObservableSlotMap::new(event_tx.clone()),
                                models: ObservableSlotMap::new(event_tx.clone()),
                                shaders: ObservableSlotMap::new(event_tx.clone()),
                                cubemaps: ObservableSlotMap::new(event_tx.clone()),

                                named_models: HashMap::new(),
                                shader_resources: ShaderResourceRegistry::new(event_tx),
                        },
                        event_rx,
                )
        }

        pub fn extend(
                &mut self,
                other: AssetStorage,
                default_sampler: SamplerId,
                default_material: MaterialId,
                default_shader: ShaderId,
        ) {
                let new_image_ids = merge_slotmaps(other.images, &mut self.images);
                let new_sampler_ids = merge_slotmaps(other.samplers, &mut self.samplers);
                let new_texture_ids = merge_slotmaps(other.textures, &mut self.textures);
                let new_material_ids = merge_slotmaps(other.materials, &mut self.materials);
                let new_mesh_ids = merge_slotmaps(other.meshes, &mut self.meshes);
                let new_model_ids = merge_slotmaps(other.models, &mut self.models);
                let new_shader_ids = merge_slotmaps(other.shaders, &mut self.shaders);
                let _new_cubemap_ids = merge_slotmaps(other.cubemaps, &mut self.cubemaps);

                for (name, &model_id) in &other.named_models {
                        match self.named_models.get(name) {
                                Some(_) => panic!("Model with name '{}' already registered!", name),
                                None => {
                                        self.named_models.insert(name.clone(), new_model_ids[model_id]);
                                },
                        }
                }

                for (_, &new_key) in &new_model_ids {
                        let model = &mut self.models[new_key];

                        model.meshes.iter_mut().for_each(|mm| {
                                mm.mesh = new_mesh_ids[mm.mesh];
                                mm.material = if mm.material != default() {
                                        new_material_ids[mm.material]
                                } else {
                                        default_material
                                }
                        });
                        model.children.iter_mut().for_each(|mid| *mid = new_model_ids[*mid]);
                }

                let default_base_color_texture = self.materials[default_material].base_color_texture;
                let default_normal_texture = self.materials[default_material].normal_texture;

                for (_, &new_key) in &new_material_ids {
                        let material = &mut self.materials[new_key];

                        material.shader = if material.shader != default() {
                                new_shader_ids[material.shader]
                        } else {
                                default_shader
                        };

                        material.base_color_texture = if material.base_color_texture != default() {
                                new_texture_ids[material.base_color_texture]
                        } else {
                                default_base_color_texture
                        };

                        material.metallic_roughness_texture = if material.metallic_roughness_texture != default() {
                                new_texture_ids[material.metallic_roughness_texture]
                        } else {
                                material.base_color_texture
                        };

                        material.normal_texture = if material.normal_texture != default() {
                                new_texture_ids[material.normal_texture]
                        } else {
                                default_normal_texture
                        };

                        material.occlusion_texture = material.occlusion_texture.map(|t| new_texture_ids[t]);
                        material.emissive_texture = material.emissive_texture.map(|t| new_texture_ids[t]);
                }

                for (_, &new_key) in &new_texture_ids {
                        let texture = &mut self.textures[new_key];

                        texture.image = new_image_ids[texture.image];
                        texture.sampler = if texture.sampler != default() {
                                new_sampler_ids[texture.sampler]
                        } else {
                                default_sampler
                        }
                }
        }
}

#[derive(Debug)]
pub struct AssetManager {
        pub assets: AssetStorage,

        shader_names: HashMap<String, ShaderId>,

        pub skybox_model: ModelId,

        default_sampler: SamplerId,
        default_material: MaterialId,
        default_shader: ShaderId,
}

impl AssetManager {
        pub fn new() -> AnyResult<(Self, Receiver<AssetManagerEvent>)> {
                let (mut assets, event_rx) = AssetStorage::new();

                assets.shader_resources.register_struct::<ShaderSettings>(
                        SHADER_RESOURCE_SHADER_SETTINGS.clone(),
                        ShaderResourceProvider::World,
                );

                assets.shader_resources.register_struct::<WorldMatrices>(
                        SHADER_RESOURCE_WORLD_MATRICES.clone(),
                        ShaderResourceProvider::World,
                );

                assets.shader_resources.register_struct::<WorldLights>(
                        SHADER_RESOURCE_WORLD_LIGHTS.clone(),
                        ShaderResourceProvider::World,
                );

                assets.shader_resources.register_struct::<BillboardData>(
                        SHADER_RESOURCE_BILLBOARD_DATA.clone(),
                        ShaderResourceProvider::World, // TODO: should be per mesh
                );

                assets.shader_resources.register_struct::<ObjectMatrices>(
                        SHADER_RESOURCE_OBJECT_MATRICES.clone(),
                        ShaderResourceProvider::Mesh,
                );

                assets.shader_resources.register_struct::<MaterialData>(
                        SHADER_RESOURCE_MATERIAL_DATA.clone(),
                        ShaderResourceProvider::Material,
                );

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_MATERIAL_BASE_COLOR_TEXTURE.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::Material,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_MATERIAL_METALLIC_ROUGHNESS_TEXTURE.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::Material,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_MATERIAL_DIFFUSE_TEXTURE.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::Material,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_MATERIAL_SPECULAR_TEXTURE.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::Material,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_MATERIAL_NORMAL_TEXTURE.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::Material,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_EQUIRECTANGULAR_MAP.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::World,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_ENVIRONMENT_MAP.clone(),
                        resource_type: ShaderResourceType::ImageCube,
                        provider: ShaderResourceProvider::World,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_IRRADIANCE_MAP.clone(),
                        resource_type: ShaderResourceType::ImageCube,
                        provider: ShaderResourceProvider::World,
                });

                assets.shader_resources.register_struct::<PrefilterParams>(
                        SHADER_RESOURCE_PREFILTER_PARAMS.clone(),
                        ShaderResourceProvider::RenderPass,
                );

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_SKYBOX.clone(),
                        resource_type: ShaderResourceType::ImageCube,
                        provider: ShaderResourceProvider::World,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_SHADOW_MAP.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::World,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_CUBE_SHADOW_MAP.clone(),
                        resource_type: ShaderResourceType::ImageCube,
                        provider: ShaderResourceProvider::World,
                });

                assets.shader_resources.register(ShaderResource {
                        id: SHADER_RESOURCE_INPUT_FRAMEBUFFER.clone(),
                        resource_type: ShaderResourceType::Image2D,
                        provider: ShaderResourceProvider::World,
                });

                let default_sampler = assets.samplers.insert(Sampler {
                        name: Some("default-sampler".into()),
                        mag_filter: MagFilter::Linear,
                        min_filter: MinFilter::LinearMipmapLinear,
                        wrap_s: WrappingMode::Repeat,
                        wrap_t: WrappingMode::Repeat,
                });

                let default_diffuse_image = assets.images.insert(Image::from_rgba_1x1(
                        Some("default-diffuse-image".into()),
                        [u8::MAX; 4],
                        ColorSpace::Srgb,
                ));

                let default_diffuse_texture = assets.textures.insert(Texture {
                        name: Some("default-diffuse-texture".into()),
                        image: default_diffuse_image,
                        sampler: default_sampler,
                });

                let default_specular_image = assets.images.insert(Image::from_rgba_1x1(
                        Some("default-specular-image".into()),
                        [u8::MAX; 4],
                        ColorSpace::Linear,
                ));

                let default_specular_texture = assets.textures.insert(Texture {
                        name: Some("default-specular-texture".into()),
                        image: default_specular_image,
                        sampler: default_sampler,
                });

                let default_normal_image = assets.images.insert(Image::from_rgba_1x1(
                        Some("default-normal-image".into()),
                        [u8::MAX / 2, u8::MAX / 2, u8::MAX, 0],
                        ColorSpace::Linear,
                ));

                let default_normal_texture = assets.textures.insert(Texture {
                        name: Some("default-normal-texture".into()),
                        image: default_normal_image,
                        sampler: default_sampler,
                });

                let default_shader = assets.shaders.insert(Shader::from_yaml(
                        &assets.shader_resources,
                        Path::new("res/shader/basic_shader/basic_shader.yaml"),
                )?);

                let default_material = assets.materials.insert(Material {
                        name: Some("default-material".into()),
                        shader: default_shader,
                        base_color_factor: Vec4::splat(1.0),
                        metallic_factor: 1.0,
                        roughness_factor: 1.0,
                        shininess: DEFAULT_SHININESS,
                        ambient_strength: DEFAULT_AMBIENT_STRENGTH,
                        specular_strength: DEFAULT_SPECULAR_STRENGTH,
                        diffuse_strength: DEFAULT_DIFFUSE_STRENGTH,
                        base_color_texture: default_diffuse_texture,
                        metallic_roughness_texture: default_specular_texture,
                        normal_texture: default_normal_texture,
                        occlusion_texture: None,
                        emissive_texture: None,
                        emissive_factor: Vec3::splat(0.0),
                });

                let skybox_shader = Shader::from_yaml(
                        &assets.shader_resources,
                        Path::new("res/shader/skybox_shader/skybox_shader.yaml"),
                )?;
                let skybox_shader = assets.shaders.insert(skybox_shader);

                let (cube_bundle, cube_model_name) = AssetBundle::from_gltf(Path::new("res/model/cube/cube.gltf"))?;
                assets.extend(cube_bundle.assets, default_sampler, default_material, default_shader);

                let cube_model = &assets.models[assets.models[assets.named_models[&cube_model_name]].children[0]];
                let cube_material_mesh = cube_model.meshes[0];
                let cube_material = &assets.materials[cube_material_mesh.material];

                let skybox_material = {
                        let mut m = cube_material.clone();
                        m.shader = skybox_shader;
                        m
                };
                let skybox_material = assets.materials.insert(skybox_material);

                let skybox_model = {
                        let mut model = cube_model.clone();
                        model.meshes[0].material = skybox_material;
                        model
                };
                let skybox_model = assets.models.insert(skybox_model);

                Ok((
                        Self {
                                assets,

                                shader_names: HashMap::new(),

                                skybox_model,

                                default_sampler,
                                default_material,
                                default_shader,
                        },
                        event_rx,
                ))
        }

        pub fn register_shader_resource(&mut self, shader_resource: ShaderResource) {
                self.assets.shader_resources.register(shader_resource);
        }

        pub fn load_shader_from_yaml(&mut self, path: &Path) -> Result<ShaderId, ShaderLoadError> {
                let shader = Shader::from_yaml(&self.assets.shader_resources, path)?;
                if self.shader_names.contains_key(&shader.name) {
                        return Err(ShaderLoadError::ShaderNameAlreadyRegistered(shader.name));
                }

                let shader_name = shader.name.clone();
                let shader_id = self.assets.shaders.insert(shader);

                debug!("Loaded shader with name: {}", shader_name);

                self.shader_names.insert(shader_name, shader_id);

                Ok(shader_id)
        }

        pub fn insert_cubemap(&mut self, cubemap: Cubemap) -> CubemapId {
                self.assets.cubemaps.insert(cubemap)
        }

        pub fn add_image(&mut self, image: Image) -> ImageId {
                self.assets.images.insert(image)
        }

        pub fn import_gltf_file(&mut self, gltf_path: &Path) -> Result<ModelId, GLTFImportError> {
                self.import_gltf_file_with_shader(gltf_path, self.default_shader)
        }

        pub fn import_gltf_file_with_shader(
                &mut self,
                gltf_path: &Path,
                shader: ShaderId,
        ) -> Result<ModelId, GLTFImportError> {
                let (bundle, root_model) = AssetBundle::from_gltf(gltf_path)?;
                self.assets
                        .extend(bundle.assets, self.default_sampler, self.default_material, shader);

                Ok(self.assets.named_models[&root_model])
        }

        pub fn get_model_by_name(&self, name: &str) -> ModelId {
                self.assets.named_models[name]
        }

        pub fn texture(&self, texture_id: TextureId) -> &Texture {
                &self.assets.textures[texture_id]
        }

        pub fn mesh(&self, mesh_id: MeshId) -> &Mesh {
                &self.assets.meshes[mesh_id]
        }

        pub fn model(&self, model_id: ModelId) -> &Model {
                &self.assets.models[model_id]
        }

        pub fn material(&self, material_id: MaterialId) -> &Material {
                &self.assets.materials[material_id]
        }

        pub fn shader(&self, shader_id: ShaderId) -> &Shader {
                &self.assets.shaders[shader_id]
        }

        pub fn get_mesh(&self, mesh_id: MeshId) -> Option<&Mesh> {
                self.assets.meshes.get(mesh_id)
        }

        pub fn get_image(&self, image_id: ImageId) -> Option<&Image> {
                self.assets.images.get(image_id)
        }

        pub fn get_sampler(&self, sampler_id: SamplerId) -> Option<&Sampler> {
                self.assets.samplers.get(sampler_id)
        }

        pub fn get_shader(&self, shader_id: ShaderId) -> Option<&Shader> {
                self.assets.shaders.get(shader_id)
        }

        // #[allow(dead_code)]
        // pub fn images(&self) -> &SlotMap<ImageId, Image> {
        //         &self.images
        // }

        // #[allow(dead_code)]
        // pub fn samplers(&self) -> &SlotMap<SamplerId, Sampler> {
        //         &self.samplers
        // }

        // #[allow(dead_code)]
        // pub fn textures(&self) -> &SlotMap<TextureId, Texture> {
        //         &self.textures
        // }

        // #[allow(dead_code)]
        // pub fn materials(&self) -> &SlotMap<MaterialId, Material> {
        //         &self.materials
        // }

        // #[allow(dead_code)]
        // pub fn iter_materials_mut(&mut self) -> impl Iterator<Item = (MaterialId, &mut Material)> {
        //         // let events = &mut self.events;
        //         self.materials.iter_mut().map(move |(mid, m)| {
        //                 // events.push(AssetManagerEvent::MaterialUpdated(mid));
        //                 (mid, m)
        //         })
        // }

        #[allow(dead_code)]
        pub fn get_material(&self, material_id: MaterialId) -> Option<&Material> {
                self.assets.materials.get(material_id)
        }

        // #[allow(dead_code)]
        // pub fn get_material_mut(&mut self, material_id: MaterialId) -> Option<&mut Material> {
        //         // self.events.push(AssetManagerEvent::MaterialUpdated(material_id));
        //         self.materials.get_mut(material_id)
        // }

        // #[allow(dead_code)]
        // pub fn meshes(&self) -> &SlotMap<MeshId, Mesh> {
        //         &self.meshes
        // }

        // #[allow(dead_code)]
        // pub fn models(&self) -> &SlotMap<ModelId, Model> {
        //         &self.models
        // }

        // #[allow(dead_code)]
        // pub fn root_models(&self) -> &HashMap<String, ModelId> {
        //         &self.root_models
        // }

        // #[allow(dead_code)]
        // pub fn shaders(&self) -> &SlotMap<ShaderId, Shader> {
        //         &self.shaders
        // }

        #[allow(dead_code)]
        pub fn shader_names(&self) -> &HashMap<String, ShaderId> {
                &self.shader_names
        }

        #[allow(dead_code)]
        pub fn shader_resources(&self) -> &ShaderResourceRegistry {
                &self.assets.shader_resources
        }

        #[allow(dead_code)]
        pub fn get_cubemap(&self, cubemap_id: CubemapId) -> Option<&Cubemap> {
                self.assets.cubemaps.get(cubemap_id)
        }
}

trait GltfElement {
        fn data_type() -> DataType;
        fn dimensions() -> Dimensions;
}

impl GltfElement for Vec2 {
        fn data_type() -> DataType {
                DataType::F32
        }

        fn dimensions() -> Dimensions {
                Dimensions::Vec2
        }
}

impl GltfElement for Vec3 {
        fn data_type() -> DataType {
                DataType::F32
        }

        fn dimensions() -> Dimensions {
                Dimensions::Vec3
        }
}

impl GltfElement for Vec4 {
        fn data_type() -> DataType {
                DataType::F32
        }

        fn dimensions() -> Dimensions {
                Dimensions::Vec4
        }
}

impl GltfElement for u16 {
        fn data_type() -> DataType {
                DataType::U16
        }

        fn dimensions() -> Dimensions {
                Dimensions::Scalar
        }
}

impl GltfElement for u32 {
        fn data_type() -> DataType {
                DataType::U32
        }

        fn dimensions() -> Dimensions {
                Dimensions::Scalar
        }
}

fn read_gltf_accessor<T: GltfElement + Clone>(buffers: &[gltf::buffer::Data], accessor: &gltf::Accessor) -> Vec<T> {
        let buffer_view = accessor.view().expect("Accessor is missing buffer view index!");
        let buffer_data = &buffers[buffer_view.buffer().index()];

        assert_eq!(accessor.data_type(), T::data_type());
        assert_eq!(accessor.dimensions(), T::dimensions());

        let byte_offset = buffer_view.offset() + accessor.offset();
        let byte_length = buffer_view.length();

        let element_count = accessor.count();

        assert!((byte_offset + byte_length) <= buffer_data.0.len());
        let data = unsafe { buffer_data.0.as_ptr().add(byte_offset) };

        assert_eq!(byte_length, element_count * std::mem::size_of::<T>());
        let slice = unsafe { std::slice::from_raw_parts(data as *const T, element_count) };

        slice.to_vec()
}

#[derive(Debug, Clone)]
pub enum AssetManagerEvent {
        ImageInserted(ImageId),
        ImageChanged(ImageId),
        ImageRemoved(ImageId),
        SamplerInserted(SamplerId),
        SamplerChanged(SamplerId),
        SamplerRemoved(SamplerId),
        TextureInserted(TextureId),
        TextureChanged(TextureId),
        TextureRemoved(TextureId),
        MaterialInserted(MaterialId),
        MaterialChanged(MaterialId),
        MaterialRemoved(MaterialId),
        MeshInserted(MeshId),
        MeshChanged(MeshId),
        MeshRemoved(MeshId),
        ModelInserted(ModelId),
        ModelChanged(ModelId),
        ModelRemoved(ModelId),
        ShaderResourceInserted(ShaderResourceId),
        ShaderResourceChanged(ShaderResourceId),
        ShaderResourceRemoved(ShaderResourceId),
        ShaderInserted(ShaderId),
        ShaderChanged(ShaderId),
        ShaderRemoved(ShaderId),
        CubemapInserted(CubemapId),
        CubemapChanged(CubemapId),
        CubemapRemoved(CubemapId),
}

fn merge_slotmaps<K: slotmap::Key, V, E: From<SlotMapEvent<K>>>(
        mut src: ObservableSlotMap<K, V, E>,
        dst: &mut ObservableSlotMap<K, V, E>,
) -> SecondaryMap<K, K> {
        let mut new_keys = SecondaryMap::new();

        for (k, v) in src.inner.drain() {
                let new_key = dst.insert(v);

                new_keys.insert(k, new_key);
        }

        new_keys
}

#[derive(Debug, Clone)]
pub struct ObservableSlotMap<K: Key, V, E: From<SlotMapEvent<K>>> {
        inner: SlotMap<K, V>,
        event_tx: crossbeam_channel::Sender<E>,
}

impl<K: Key, V, E: From<SlotMapEvent<K>>> ObservableSlotMap<K, V, E> {
        pub fn new(event_tx: crossbeam_channel::Sender<E>) -> Self {
                Self {
                        inner: SlotMap::with_key(),
                        event_tx,
                }
        }

        #[allow(dead_code)]
        pub fn insert(&mut self, v: V) -> K {
                let k = self.inner.insert(v);
                self.event_tx.send(SlotMapEvent::Inserted(k).into()).unwrap();
                self.event_tx.send(SlotMapEvent::Changed(k).into()).unwrap();
                k
        }

        #[allow(dead_code)]
        pub fn remove(&mut self, k: K) -> Option<V> {
                let v = self.inner.remove(k);
                self.event_tx.send(SlotMapEvent::Removed(k).into()).unwrap();
                v
        }

        #[allow(dead_code)]
        pub fn get(&self, k: K) -> Option<&V> {
                self.inner.get(k)
        }

        #[allow(dead_code)]
        pub fn get_mut(&mut self, k: K) -> Option<&mut V> {
                self.event_tx.send(SlotMapEvent::Changed(k).into()).unwrap();
                self.inner.get_mut(k)
        }
}

impl<K: Key, V, E: From<SlotMapEvent<K>>> Index<K> for ObservableSlotMap<K, V, E> {
        type Output = V;

        fn index(&self, index: K) -> &Self::Output {
                &self.inner[index]
        }
}

impl<K: Key, V, E: From<SlotMapEvent<K>>> IndexMut<K> for ObservableSlotMap<K, V, E> {
        fn index_mut(&mut self, index: K) -> &mut Self::Output {
                self.event_tx.send(SlotMapEvent::Changed(index).into()).unwrap();
                &mut self.inner[index]
        }
}

impl<'a, K: Key, V, E: From<SlotMapEvent<K>>> IntoIterator for &'a ObservableSlotMap<K, V, E> {
        type Item = (K, &'a V);
        type IntoIter = slotmap::basic::Iter<'a, K, V>;

        fn into_iter(self) -> Self::IntoIter {
                self.inner.iter()
        }
}

// impl<'a, K: Key, V, E: From<SlotMapEvent<K>>> IntoIterator for &'a mut ObservableSlotMap<K, V, E> {
//         type Item = (K, &'a mut V);
//         type IntoIter = impl Iterator<Item = (K, &'a mut V)> + '_;

//         fn into_iter(self) -> Self::IntoIter {
//                 self.inner.iter_mut().map(|(k, v)| {
//                         self.event_tx.send(SlotMapEvent::Changed(k).into());
//                         (k, v)
//                 })
//         }
// }

pub enum SlotMapEvent<K: Key> {
        Inserted(K),
        Changed(K),
        Removed(K),
}

impl From<SlotMapEvent<ImageId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<ImageId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::ImageInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::ImageChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::ImageRemoved(id),
                }
        }
}

impl From<SlotMapEvent<SamplerId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<SamplerId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::SamplerInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::SamplerChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::SamplerRemoved(id),
                }
        }
}

impl From<SlotMapEvent<TextureId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<TextureId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::TextureInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::TextureChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::TextureRemoved(id),
                }
        }
}

impl From<SlotMapEvent<MaterialId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<MaterialId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::MaterialInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::MaterialChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::MaterialRemoved(id),
                }
        }
}

impl From<SlotMapEvent<MeshId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<MeshId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::MeshInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::MeshChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::MeshRemoved(id),
                }
        }
}

impl From<SlotMapEvent<ModelId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<ModelId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::ModelInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::ModelChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::ModelRemoved(id),
                }
        }
}

impl From<SlotMapEvent<ShaderId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<ShaderId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::ShaderInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::ShaderChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::ShaderRemoved(id),
                }
        }
}

impl From<SlotMapEvent<CubemapId>> for AssetManagerEvent {
        fn from(e: SlotMapEvent<CubemapId>) -> Self {
                match e {
                        SlotMapEvent::Inserted(id) => AssetManagerEvent::CubemapInserted(id),
                        SlotMapEvent::Changed(id) => AssetManagerEvent::CubemapChanged(id),
                        SlotMapEvent::Removed(id) => AssetManagerEvent::CubemapRemoved(id),
                }
        }
}

pub enum ObservableEvent<K> {
        Inserted(K),
        Updated(K),
        Removed(K),
}

pub trait ObservableMap<K, V> {
        fn insert(&mut self, key: K, value: V) -> Option<V>;
        fn remove(&mut self, key: &K) -> Option<V>;
        fn get(&self, key: &K) -> Option<&V>;
        // fn get_mut(&mut self, key: K) -> Option<&mut V>;
}

#[derive(Debug, Clone)]
pub struct Observable<K: Clone, V, E: From<ObservableEvent<K>>, T: ObservableMap<K, V>> {
        inner: T,
        event_tx: crossbeam_channel::Sender<E>,
        key_type: PhantomData<K>,
        value_type: PhantomData<V>,
}

impl<K: Clone, V, E: From<ObservableEvent<K>>, T: ObservableMap<K, V>> Observable<K, V, E, T> {
        pub fn new(map: T, tx: crossbeam_channel::Sender<E>) -> Self {
                Self {
                        inner: map,
                        event_tx: tx,
                        key_type: PhantomData,
                        value_type: PhantomData,
                }
        }

        #[allow(dead_code)]
        pub fn insert(&mut self, k: K, v: V) -> Option<V> {
                let u = self.inner.insert(k.clone(), v);

                let e = if u.is_none() {
                        ObservableEvent::Inserted(k)
                } else {
                        ObservableEvent::Updated(k)
                };
                self.event_tx.send(e.into()).unwrap();

                u
        }

        #[allow(dead_code)]
        pub fn remove(&mut self, k: &K) -> Option<V> {
                let v = self.inner.remove(k);
                self.event_tx.send(ObservableEvent::Removed(k.clone()).into()).unwrap();
                v
        }

        #[allow(dead_code)]
        pub fn get(&self, k: &K) -> Option<&V> {
                self.inner.get(k)
        }

        // #[allow(dead_code)]
        // pub fn get_mut(&mut self, k: K) -> Option<&mut V> {
        //         self.event_tx.send(ObservableEvent::Updated(k).into()).unwrap();
        //         self.inner.get_mut(k)
        // }
}

impl<K: Eq + Hash, V> ObservableMap<K, V> for HashMap<K, V> {
        fn insert(&mut self, k: K, v: V) -> Option<V> {
                HashMap::insert(self, k, v)
        }

        fn remove(&mut self, k: &K) -> Option<V> {
                HashMap::remove(self, k)
        }

        fn get(&self, k: &K) -> Option<&V> {
                HashMap::get(self, k)
        }

        // fn get_mut(&mut self, key: K) -> Option<&mut V> {
        //         todo!()
        // }
}

impl From<ObservableEvent<ShaderResourceId>> for AssetManagerEvent {
        fn from(e: ObservableEvent<ShaderResourceId>) -> Self {
                match e {
                        ObservableEvent::Inserted(id) => AssetManagerEvent::ShaderResourceInserted(id),
                        ObservableEvent::Updated(id) => AssetManagerEvent::ShaderResourceChanged(id),
                        ObservableEvent::Removed(id) => AssetManagerEvent::ShaderResourceRemoved(id),
                }
        }
}

fn compute_dds_mipmap_stride(width: u32, height: u32, format: &Box<dyn ddsfile::DataFormat>) -> u32 {
        let pitch = format.get_pitch(width).expect("format does not provide pitch");
        let pitch_height = format.get_pitch_height();
        // round up to align with pitch height
        let rows = (height + (pitch_height - 1)) / pitch_height;

        rows * pitch
}
