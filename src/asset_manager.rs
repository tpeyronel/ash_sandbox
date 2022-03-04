use std::{
        ffi::OsString,
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
};

use gltf::image::Format;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use slotmap::SlotMap;
use thiserror::Error;

use crate::{
        components::Transform,
        constants::{DEFAULT_AMBIENT_STRENGTH, DEFAULT_DIFFUSE_STRENGTH, DEFAULT_SHININESS, DEFAULT_SPECULAR_STRENGTH},
        hashmap::HashMap,
        my_glm::*,
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

pub type ComponentType = gltf::accessor::DataType;
pub type DataType = gltf::accessor::Dimensions;

slotmap::new_key_type! { pub struct ModelId; }

#[derive(Debug, Clone)]
pub struct Model {
        pub name: Option<String>,
        pub base_transform: Transform,
        pub meshes: Vec<MeshId>,
        pub children: Vec<ModelId>,
}

pub struct MeshGroup(Vec<MeshId>);

slotmap::new_key_type! { pub struct MeshId; }

#[derive(Debug, Clone)]
pub struct Mesh {
        pub positions: BufferViewId,
        pub tex_coords: BufferViewId,
        pub normals: BufferViewId,
        pub tangents: BufferViewId,
        pub indices: BufferViewId,
        pub material: MaterialId,
        pub bounding_box: BoundingBox,
}

slotmap::new_key_type! { pub struct BufferViewId; }

#[derive(Debug, Clone)]
pub struct BufferView {
        pub buffer: BufferId,
        pub byte_length: usize,
        pub byte_offset: usize,
        pub component_type: ComponentType,
        pub data_type: DataType,
        pub element_count: usize,
}

slotmap::new_key_type! { pub struct BufferId; }

#[derive(Debug, Clone)]
pub struct Buffer {
        pub bytes: Arc<Vec<u8>>,
}

impl Buffer {
        fn new(bytes: Vec<u8>) -> Self {
                Self { bytes: Arc::new(bytes) }
        }
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
        pub normal_texture: Option<TextureId>,
        pub occlusion_texture: Option<TextureId>,
        pub emissive_texture: Option<TextureId>,
        pub emissive_factor: Vec3,
}

slotmap::new_key_type! { pub struct TextureId; }

#[derive(Debug, Clone)]
pub struct Texture {
        pub name: Option<String>,
        pub image: ImageId,
        pub sampler: SamplerId,
}

pub type ImageFormat = gltf::image::Format;

slotmap::new_key_type! { pub struct ImageId; }

#[derive(Debug, Clone)]
pub struct Image {
        // name: Option<String>,
        pub pixels: BufferId,
        pub width: u32,
        pub height: u32,
        pub format: ImageFormat,
}

pub type MagFilter = gltf::texture::MagFilter;
pub type MinFilter = gltf::texture::MinFilter;
pub type WrappingMode = gltf::texture::WrappingMode;

slotmap::new_key_type! { pub struct SamplerId; }

#[derive(Debug, Clone)]
pub struct Sampler {
        pub name: Option<String>,
        pub mag_filter: MagFilter,
        pub min_filter: MinFilter,
        pub wrap_s: WrappingMode,
        pub wrap_t: WrappingMode,
}

#[derive(Debug, serde::Deserialize)]
pub struct ShaderDeclaration {
        pub name: String,

        #[serde(rename = "vertex-shader")]
        pub vert_shader: PathBuf,

        #[serde(rename = "fragment-shader")]
        pub frag_shader: PathBuf,

        pub uniforms: Vec<String>,

        #[serde(rename = "vertex-inputs")]
        pub vertex_inputs: Vec<String>,
}

pub type ShaderResourceId = String;

slotmap::new_key_type! { pub struct ShaderId; }

#[derive(Debug, Clone)]
pub struct Shader {
        pub name: String,
        pub vert_module: Arc<ShaderModule>,
        pub frag_module: Arc<ShaderModule>,
        pub resources: Vec<ShaderResourceId>,
        pub vertex_inputs: Vec<String>,
}

impl Shader {
        pub fn from_yaml(path: &Path) -> Result<Self, ShaderLoadError> {
                let yaml = std::fs::read_to_string(path)?;
                let declaration: ShaderDeclaration = serde_yaml::from_str(&yaml)?;

                let directory = path
                        .parent()
                        .ok_or_else(|| ShaderLoadError::InvalidPath(format!("Path {:?} does not have parent", path)))?;

                Ok(Self {
                        name: declaration.name.clone(),
                        vert_module: Arc::new(ShaderModule::from_glsl_file(directory.join(&declaration.vert_shader))?),
                        frag_module: Arc::new(ShaderModule::from_glsl_file(directory.join(&declaration.frag_shader))?),
                        resources: declaration.uniforms,
                        vertex_inputs: declaration.vertex_inputs,
                })
        }
}

#[derive(Debug)]
pub struct ShaderModule {
        pub bin: Vec<u8>,
}

impl ShaderModule {
        fn from_glsl_file(path: PathBuf) -> Result<Self, ShaderLoadError> {
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

#[derive(Debug, Clone)]
pub struct ShaderResource {
        pub elements: Vec<ShaderResourceElement>,
}

#[derive(Debug, Clone, Copy)]
pub struct ShaderResourceElement {
        pub element_type: ShaderResourceElementType,
        pub shader_stage_flags: ash::vk::ShaderStageFlags,
}

#[derive(Debug, Clone, Copy)]
pub enum ShaderResourceElementType {
        Sampler,
        SampledImage,
        UniformBuffer,
        StorageBuffer,
        UniformBufferDynamic,
        StorageBufferDynamic,
}

impl From<ShaderResourceElementType> for ash::vk::DescriptorType {
        fn from(t: ShaderResourceElementType) -> Self {
                match t {
                        ShaderResourceElementType::Sampler => ash::vk::DescriptorType::SAMPLER,
                        ShaderResourceElementType::SampledImage => ash::vk::DescriptorType::SAMPLED_IMAGE,
                        ShaderResourceElementType::UniformBuffer => ash::vk::DescriptorType::UNIFORM_BUFFER,
                        ShaderResourceElementType::StorageBuffer => ash::vk::DescriptorType::STORAGE_BUFFER,
                        ShaderResourceElementType::UniformBufferDynamic => {
                                ash::vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC
                        },
                        ShaderResourceElementType::StorageBufferDynamic => {
                                ash::vk::DescriptorType::STORAGE_BUFFER_DYNAMIC
                        },
                }
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
        #[error("image format not supported")]
        ImageFormatNotSupported,
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
}

impl From<OsString> for ShaderLoadError {
        fn from(s: OsString) -> Self {
                Self::InvalidUnicode(s)
        }
}

/* pub struct AssetManagerBuilder {
        gltf_paths: Vec<PathBuf>,
}

impl AssetManagerBuilder {
        pub fn new() -> Self {
                Self { gltf_paths: Vec::new() }
        }
} */

#[derive(Debug, Clone)]
pub struct AssetManager {
        events: Vec<AssetManagerEvent>,
        buffers: SlotMap<BufferId, Buffer>,
        buffer_views: SlotMap<BufferViewId, BufferView>,
        images: SlotMap<ImageId, Image>,
        samplers: SlotMap<SamplerId, Sampler>,
        textures: SlotMap<TextureId, Texture>,
        materials: SlotMap<MaterialId, Material>,
        meshes: SlotMap<MeshId, Mesh>,
        models: SlotMap<ModelId, Model>,
        root_models: HashMap<String, ModelId>,

        shader_resources: HashMap<ShaderResourceId, ShaderResource>,
        shaders: SlotMap<ShaderId, Shader>,
        shader_names: HashMap<String, ShaderId>,

        default_sampler: SamplerId,
        default_material: MaterialId,
}

impl AssetManager {
        pub fn new() -> AnyResult<Self> {
                let mut events = Vec::new();
                let mut buffers = SlotMap::with_key();
                let buffer_views = SlotMap::with_key();
                let mut images = SlotMap::with_key();
                let mut samplers = SlotMap::with_key();
                let mut textures = SlotMap::with_key();
                let mut materials = SlotMap::with_key();
                let meshes = SlotMap::with_key();
                let models = SlotMap::with_key();
                let mut shaders = SlotMap::with_key();

                let default_sampler = samplers.insert(Sampler {
                        name: Some("default-sampler".into()),
                        mag_filter: MagFilter::Linear,
                        min_filter: MinFilter::LinearMipmapLinear,
                        wrap_s: WrappingMode::Repeat,
                        wrap_t: WrappingMode::Repeat,
                });
                events.push(AssetManagerEvent::SamplerUpdated(default_sampler));

                let default_diffuse_image_buffer = buffers.insert(Buffer::new(vec![u8::MAX; 4]));

                let default_diffuse_image = images.insert(Image {
                        pixels: default_diffuse_image_buffer,
                        width: 1,
                        height: 1,
                        format: Format::R8G8B8A8,
                });
                events.push(AssetManagerEvent::ImageUpdated(default_diffuse_image));

                let default_diffuse_texture = textures.insert(Texture {
                        name: Some("default-diffuse-texture".into()),
                        image: default_diffuse_image,
                        sampler: default_sampler,
                });

                let default_specular_image_buffer = buffers.insert(Buffer::new(vec![u8::MAX; 4]));

                let default_specular_image = images.insert(Image {
                        pixels: default_specular_image_buffer,
                        width: 1,
                        height: 1,
                        format: Format::R8G8B8A8,
                });
                events.push(AssetManagerEvent::ImageUpdated(default_specular_image));

                let default_specular_texture = textures.insert(Texture {
                        name: Some("default-specular-texture".into()),
                        image: default_specular_image,
                        sampler: default_sampler,
                });

                let default_shader = shaders.insert(Shader::from_yaml(Path::new(
                        "res/shader/basic_shader/basic_shader.yaml",
                ))?);
                events.push(AssetManagerEvent::ShaderUpdated(default_shader));

                let default_material = materials.insert(Material {
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
                        normal_texture: None,
                        occlusion_texture: None,
                        emissive_texture: None,
                        emissive_factor: Vec3::splat(0.0),
                });
                events.push(AssetManagerEvent::MaterialUpdated(default_material));

                Ok(Self {
                        events,
                        buffers,
                        buffer_views,
                        images,
                        samplers,
                        textures,
                        materials,
                        meshes,
                        models,
                        root_models: HashMap::new(),

                        shader_resources: HashMap::new(),
                        shaders,
                        shader_names: HashMap::new(),

                        default_sampler,
                        default_material,
                })
        }

        pub fn events(&self) -> &Vec<AssetManagerEvent> {
                &self.events
        }

        pub fn clear_events(&mut self) {
                self.events.clear();
        }

        pub fn register_shader_resource(
                &mut self,
                shader_resource_id: ShaderResourceId,
                shader_resource: ShaderResource,
        ) {
                self.shader_resources.insert(shader_resource_id, shader_resource);
        }

        pub fn load_shader_from_yaml(&mut self, path: &Path) -> Result<ShaderId, ShaderLoadError> {
                let shader = Shader::from_yaml(path)?;
                if self.shader_names.contains_key(&shader.name) {
                        return Err(ShaderLoadError::ShaderNameAlreadyRegistered(shader.name));
                }

                let shader_name = shader.name.clone();
                let shader_id = self.shaders.insert(shader);

                debug!("Loaded shader with name: {}", shader_name);

                self.shader_names.insert(shader_name, shader_id);

                self.events.push(AssetManagerEvent::ShaderUpdated(shader_id));

                Ok(shader_id)
        }

        pub fn import_gltf_file(&mut self, gltf_path: &Path) -> Result<ModelId, GLTFImportError> {
                scoped_timer!("Loaded model in ", Millis);

                let (doc, buffers, images) = gltf::import(gltf_path)?;

                let buffers = Self::load_buffers(buffers, &mut self.buffers);
                let buffer_view_ids =
                        Self::load_buffer_views(&doc, &buffers, &mut self.events, &mut self.buffer_views)?;
                let image_ids = Self::load_images(
                        gltf_path,
                        &doc,
                        images,
                        &mut self.events,
                        &mut self.buffers,
                        &mut self.images,
                )?;
                let sampler_ids = Self::load_samplers(&doc, self.default_sampler, &mut self.events, &mut self.samplers);
                let texture_ids =
                        Self::load_textures(&doc, &image_ids, &sampler_ids, self.default_sampler, &mut self.textures);
                let material_ids = Self::load_materials(
                        &doc,
                        &self.shader_names,
                        &texture_ids,
                        &mut self.events,
                        &mut self.materials,
                        self.default_material,
                );
                let mesh_groups = Self::load_meshes(
                        &doc,
                        &buffer_view_ids,
                        &material_ids,
                        self.default_material,
                        &mut self.meshes,
                )?;
                let model_ids = Self::load_models(&doc, &mesh_groups, &mut self.models)?;
                let root_model_id = Self::load_root_model(&doc, &model_ids, &mut self.models, &mut self.root_models)?;

                Ok(root_model_id)
        }

        pub fn get_model_by_name(&self, name: &str) -> ModelId {
                *self.root_models.get(name).unwrap()
        }

        #[allow(dead_code)]
        pub fn buffers(&self) -> &SlotMap<BufferId, Buffer> {
                &self.buffers
        }

        #[allow(dead_code)]
        pub fn buffer_views(&self) -> &SlotMap<BufferViewId, BufferView> {
                &self.buffer_views
        }

        #[allow(dead_code)]
        pub fn images(&self) -> &SlotMap<ImageId, Image> {
                &self.images
        }

        #[allow(dead_code)]
        pub fn samplers(&self) -> &SlotMap<SamplerId, Sampler> {
                &self.samplers
        }

        #[allow(dead_code)]
        pub fn textures(&self) -> &SlotMap<TextureId, Texture> {
                &self.textures
        }

        #[allow(dead_code)]
        pub fn materials(&self) -> &SlotMap<MaterialId, Material> {
                &self.materials
        }

        #[allow(dead_code)]
        pub fn iter_materials_mut(&mut self) -> impl Iterator<Item = (MaterialId, &mut Material)> {
                // let events = &mut self.events;
                self.materials.iter_mut().map(move |(mid, m)| {
                        // events.push(AssetManagerEvent::MaterialUpdated(mid));
                        (mid, m)
                })
        }

        #[allow(dead_code)]
        pub fn get_material(&self, material_id: MaterialId) -> Option<&Material> {
                self.materials.get(material_id)
        }

        #[allow(dead_code)]
        pub fn get_material_mut(&mut self, material_id: MaterialId) -> Option<&mut Material> {
                // self.events.push(AssetManagerEvent::MaterialUpdated(material_id));
                self.materials.get_mut(material_id)
        }

        #[allow(dead_code)]
        pub fn meshes(&self) -> &SlotMap<MeshId, Mesh> {
                &self.meshes
        }

        #[allow(dead_code)]
        pub fn models(&self) -> &SlotMap<ModelId, Model> {
                &self.models
        }

        #[allow(dead_code)]
        pub fn root_models(&self) -> &HashMap<String, ModelId> {
                &self.root_models
        }

        #[allow(dead_code)]
        pub fn shaders(&self) -> &SlotMap<ShaderId, Shader> {
                &self.shaders
        }

        #[allow(dead_code)]
        pub fn shader_names(&self) -> &HashMap<String, ShaderId> {
                &self.shader_names
        }

        #[allow(dead_code)]
        pub fn shader_resources(&self) -> &HashMap<ShaderResourceId, ShaderResource> {
                &self.shader_resources
        }

        fn load_buffers(
                buffers_data: Vec<gltf::buffer::Data>,
                out_buffers: &mut SlotMap<BufferId, Buffer>,
        ) -> Vec<BufferId> {
                buffers_data
                        .into_iter()
                        .map(|b| out_buffers.insert(Buffer::new(b.0)))
                        .collect()
        }

        fn load_buffer_views(
                doc: &gltf::Document,
                buffers: &Vec<BufferId>,
                out_events: &mut Vec<AssetManagerEvent>,
                out_buffer_views: &mut SlotMap<BufferViewId, BufferView>,
        ) -> Result<Vec<BufferViewId>, GLTFImportError> {
                doc.accessors()
                        .map(|a| {
                                let bview = match a.view() {
                                        Some(bview) => bview,
                                        None => {
                                                error!("Accessor is missing buffer view index!");
                                                return Err(GLTFImportError::AccessorMissingBufferView);
                                        },
                                };

                                let buffer_view_id = out_buffer_views.insert(BufferView {
                                        buffer: buffers[bview.buffer().index()],
                                        byte_length: bview.length(),
                                        byte_offset: bview.offset() + a.offset(),
                                        component_type: a.data_type(),
                                        data_type: a.dimensions(),
                                        element_count: a.count(),
                                });
                                out_events.push(AssetManagerEvent::BufferViewUpdated(buffer_view_id));

                                Ok(buffer_view_id)
                        })
                        .collect()
        }

        fn load_images(
                gltf_path: &Path,
                doc: &gltf::Document,
                images: Vec<gltf::image::Data>,
                out_events: &mut Vec<AssetManagerEvent>,
                out_buffers: &mut SlotMap<BufferId, Buffer>,
                out_images: &mut SlotMap<ImageId, Image>,
        ) -> Result<Vec<ImageId>, GLTFImportError> {
                images.into_iter()
                        .zip(doc.images())
                        .filter_map(|(image, json_image)| {
                                if !Self::is_image_format_supported(image.format) {
                                        return Some(Err(GLTFImportError::ImageFormatNotSupported));
                                }

                                // Image path relative to working directory
                                let _image_relative_path = match json_image.source() {
                                        gltf::image::Source::Uri { uri, .. } => {
                                                if uri.contains(":") {
                                                        error!("Trying to import image with non relative uri!");
                                                        return Some(Err(GLTFImportError::ImageSourceUriNotRelative));
                                                }

                                                gltf_path.join(uri)
                                        },
                                        _ => {
                                                error!("Image source is not an uri!");
                                                return Some(Err(GLTFImportError::ImageSourceNotUri));
                                        },
                                };

                                let pixels_buffer_id = out_buffers.insert(Buffer::new(image.pixels));

                                let image_id = out_images.insert(Image {
                                        pixels: pixels_buffer_id,
                                        width: image.width,
                                        height: image.height,
                                        format: image.format,
                                });
                                out_events.push(AssetManagerEvent::ImageUpdated(image_id));

                                Some(Ok(image_id))
                        })
                        .collect()
        }

        fn is_image_format_supported(format: gltf::image::Format) -> bool {
                type Format = gltf::image::Format;

                match format {
                        Format::R8 => true,
                        Format::R8G8B8 => true,
                        Format::R8G8B8A8 => true,
                        _ => false,
                }
        }

        fn load_samplers(
                doc: &gltf::Document,
                default_sampler: SamplerId,
                out_events: &mut Vec<AssetManagerEvent>,
                out_samplers: &mut SlotMap<SamplerId, Sampler>,
        ) -> Vec<SamplerId> {
                doc.samplers()
                        .map(|s| {
                                let sampler_id = out_samplers.insert(Sampler {
                                        name: s.name().map(String::from),
                                        mag_filter: s.mag_filter().unwrap_or(out_samplers[default_sampler].mag_filter),
                                        min_filter: s.min_filter().unwrap_or(out_samplers[default_sampler].min_filter),
                                        wrap_s: s.wrap_s(),
                                        wrap_t: s.wrap_t(),
                                });

                                out_events.push(AssetManagerEvent::SamplerUpdated(sampler_id));

                                sampler_id
                        })
                        .collect()
        }

        fn load_textures(
                doc: &gltf::Document,
                image_ids: &Vec<ImageId>,
                sampler_ids: &Vec<SamplerId>,
                default_sampler: SamplerId,
                out_textures: &mut SlotMap<TextureId, Texture>,
        ) -> Vec<TextureId> {
                doc.textures()
                        .map(|t| {
                                let tex_id = out_textures.insert(Texture {
                                        name: t.name().map(String::from),
                                        image: image_ids[t.source().index()],
                                        sampler: t.sampler().index().map_or(default_sampler, |i| sampler_ids[i]),
                                });

                                tex_id
                        })
                        .collect()
        }

        fn load_materials(
                doc: &gltf::Document,
                shader_names: &HashMap<String, ShaderId>,
                texture_ids: &Vec<TextureId>,
                out_events: &mut Vec<AssetManagerEvent>,
                out_materials: &mut SlotMap<MaterialId, Material>,
                default_material: MaterialId,
        ) -> Vec<MaterialId> {
                doc.materials()
                        .map(|m| {
                                let default_material = &out_materials[default_material];

                                // TODO: handle textures better
                                let pbr_mr = m.pbr_metallic_roughness();

                                let base_color_factor = pbr_mr.base_color_factor().into();
                                let metallic_factor = pbr_mr.metallic_factor();
                                let roughness_factor = pbr_mr.roughness_factor();
                                let base_color_texture = pbr_mr
                                        .base_color_texture()
                                        .map(|t| texture_ids[t.texture().index()])
                                        .unwrap_or(default_material.base_color_texture);
                                let metallic_roughness_texture = pbr_mr
                                        .metallic_roughness_texture()
                                        .map(|t| texture_ids[t.texture().index()])
                                        .unwrap_or(base_color_texture);
                                let normal_texture = m.normal_texture().map(|t| texture_ids[t.texture().index()]);
                                let occlusion_texture = m.occlusion_texture().map(|t| texture_ids[t.texture().index()]);
                                let emissive_texture = m.emissive_texture().map(|t| texture_ids[t.texture().index()]);
                                let emissive_factor = Vec3::from_slice(&m.emissive_factor());

                                let shader = if emissive_factor.length_squared() == 0.0 {
                                        shader_names["basic-shader"]
                                } else {
                                        shader_names["color-shader"]
                                };

                                let mat_id = out_materials.insert(Material {
                                        name: m.name().map(String::from),
                                        shader,
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
                                });

                                out_events.push(AssetManagerEvent::MaterialUpdated(mat_id));

                                mat_id
                        })
                        .collect()
        }

        fn load_meshes(
                doc: &gltf::Document,
                buffer_view_ids: &Vec<BufferViewId>,
                material_ids: &Vec<MaterialId>,
                default_material: MaterialId,
                out_meshes: &mut SlotMap<MeshId, Mesh>,
        ) -> Result<Vec<MeshGroup>, GLTFImportError> {
                let mut mesh_groups = Vec::new();

                for m in doc.meshes() {
                        let mut mesh_group = MeshGroup(Vec::new());

                        for p in m.primitives() {
                                let positions = match p.get(&gltf::Semantic::Positions) {
                                        Some(positions) => buffer_view_ids[positions.index()],
                                        None => return Err(GLTFImportError::MeshMissingPositions),
                                };

                                let tex_coords = match p.get(&gltf::Semantic::TexCoords(0)) {
                                        Some(tex_coords) => buffer_view_ids[tex_coords.index()],
                                        None => return Err(GLTFImportError::MeshMissingTexCoords),
                                };

                                let normals = match p.get(&gltf::Semantic::Normals) {
                                        Some(normals) => buffer_view_ids[normals.index()],
                                        None => return Err(GLTFImportError::MeshMissingNormals),
                                };

                                let tangents = match p.get(&gltf::Semantic::Tangents) {
                                        Some(tangents) => buffer_view_ids[tangents.index()],
                                        None => return Err(GLTFImportError::MeshMissingTangents),
                                };

                                let indices = match p.indices() {
                                        Some(indices) => buffer_view_ids[indices.index()],
                                        None => return Err(GLTFImportError::MeshMissingIndices),
                                };

                                let material = p.material().index().map_or(default_material, |i| material_ids[i]);

                                let mesh = Mesh {
                                        positions,
                                        tex_coords,
                                        normals,
                                        tangents,
                                        indices,
                                        material,
                                        bounding_box: BoundingBox::from(&p.bounding_box()),
                                };

                                let mesh_id = out_meshes.insert(mesh);
                                mesh_group.0.push(mesh_id);
                        }

                        mesh_groups.push(mesh_group);
                }

                Ok(mesh_groups)
        }

        fn load_models(
                doc: &gltf::Document,
                mesh_groups: &Vec<MeshGroup>,
                out_models: &mut SlotMap<ModelId, Model>,
        ) -> Result<Vec<ModelId>, GLTFImportError> {
                let mut model_ids = Vec::new();

                for n in doc.nodes() {
                        let meshes =
                                n.mesh().map(|mg| mesh_groups[mg.index()].0.clone())
                                        .unwrap_or_else(|| Vec::new());

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
                model_ids: &Vec<ModelId>,
                out_models: &mut SlotMap<ModelId, Model>,
                out_root_models: &mut HashMap<String, ModelId>,
        ) -> Result<ModelId, GLTFImportError> {
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

                if out_root_models.contains_key(&name) {
                        return Err(GLTFImportError::SceneNameAlreadyRegistered);
                }

                let children = scene.nodes().map(|n| model_ids[n.index()]).collect();

                let model = Model {
                        name: Some(name.clone()),
                        base_transform: Transform::from_rotation(Quat::from_axis_angle(
                                Vec3::UP,
                                180.0f32.to_radians(),
                        )),
                        meshes: Vec::new(),
                        children,
                };
                let model_id = out_models.insert(model);

                out_root_models.insert(name, model_id);

                Ok(model_id)
        }

        /* fn create_model_from_node_recursively(
                n: &gltf::Node,
                buffer_view_ids: &Vec<BufferViewID>,
                material_ids: &Vec<MaterialID>,
                default_material: MaterialID,
                out_models: &mut SlotMap<ModelID, Model>,
        ) -> Option<Result<ModelID, GLTFImportError>> {
                let m = match n.mesh() {
                        Some(m) => m,
                        None => return None,
                };



                let meshes = match meshes {
                        Ok(meshes) => meshes,
                        Err(e) => return Some(Err(e)),
                };

                let children = n
                        .children()
                        .filter_map(|n| {
                                Self::create_model_from_node_recursively(
                                        &n,
                                        buffer_view_ids,
                                        material_ids,
                                        default_material,
                                        out_models,
                                )
                        })
                        .collect::<Result<Vec<ModelID>, GLTFImportError>>();

                let children = match children {
                        Ok(children) => children,
                        Err(e) => return Some(Err(e)),
                };

                let transform = match n.transform() {
                        gltf::scene::Transform::Matrix { matrix } => unsafe {
                                na::Matrix4::from_column_slice(std::slice::from_raw_parts(
                                        &matrix as *const _ as *const f32,
                                        16,
                                ))
                        },
                        gltf::scene::Transform::Decomposed {
                                ref translation,
                                ref rotation,
                                ref scale,
                        } => {
                                let t = Mat4::from_translation(&Vec3::from_slice(translation));

                                let r = Quat::new_unchecked(Quat::new(
                                        rotation[3],
                                        rotation[0],
                                        rotation[1],
                                        rotation[2],
                                ));

                                let s = Mat4::new_nonuniform_scaling(&Vec3::from_slice(scale));

                                t * r.to_homogeneous() * s
                        }
                };

                let model_id = out_models.insert(Model {
                        name: n.name().map(String::from),
                        transform,
                        meshes,
                        children,
                });

                Some(Ok(model_id))
        } */
}

#[derive(Debug, Clone)]
pub enum AssetManagerEvent {
        // BufferCreated(BufferId),
        // BufferDeleted(BufferId),
        BufferViewUpdated(BufferViewId),
        BufferViewDeleted(BufferViewId),
        ImageUpdated(ImageId),
        ImageDeleted(ImageId),
        SamplerUpdated(SamplerId),
        SamplerDeleted(SamplerId),
        // TextureUpdated(TextureId),
        // TextureDeleted(TextureId),
        MaterialUpdated(MaterialId),
        MaterialDeleted(MaterialId),
        // MeshUpdated(MeshId),
        // MeshDeleted(MeshId),
        // ModelUpdated(ModelId),
        // ModelDeleted(ModelId),
        ShaderUpdated(ShaderId),
        ShaderDeleted(ShaderId),
}
