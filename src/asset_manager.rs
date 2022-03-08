use std::{
        ffi::OsString,
        ops::{Index, IndexMut},
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
};

use crossbeam_channel::Receiver;
use gltf::{
        accessor::{DataType, Dimensions},
        image::Format,
};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use serde::Deserialize;
use slotmap::{Key, SecondaryMap, SlotMap};
use thiserror::Error;

use crate::{
        components::Transform,
        constants::{
                DEFAULT_AMBIENT_STRENGTH, DEFAULT_DIFFUSE_STRENGTH, DEFAULT_MAG_FILTER, DEFAULT_MIN_FILTER,
                DEFAULT_SHININESS, DEFAULT_SPECULAR_STRENGTH,
        },
        hashmap::HashMap,
        my_glm::*,
        util::{default, log_if_error},
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
        pub meshes: Vec<MeshId>,
        pub children: Vec<ModelId>,
}

pub struct MeshGroup(Vec<MeshId>);

slotmap::new_key_type! { pub struct MeshId; }

#[derive(Debug, Clone)]
pub struct Mesh {
        pub positions: Vec<Vec3>,
        pub tex_coords: Vec<Vec2>,
        pub normals: Vec<Vec3>,
        pub tangents: Vec<Vec4>,
        pub indices: IndicesVec,
        pub material: MaterialId,
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
        pub pixels: Vec<u8>,
        pub width: u32,
        pub height: u32,
        pub format: ImageFormat,
}

impl Image {
        pub fn from_file(path: &Path) -> AnyResult<Self> {
                let image = image::open(path)?;

                match image {
                        image::DynamicImage::ImageRgba8(_) => (),
                        _ => panic!("Unsupported image format!"),
                }

                Ok(Self {
                        width: image.width(),
                        height: image.height(),
                        pixels: image.into_bytes(),
                        format: ImageFormat::R8G8B8A8,
                })
        }
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
        pub frag_shader: PathBuf,

        // default is false
        #[serde(rename = "disable-depth-test", default)]
        pub disable_depth_test: bool,

        #[serde(rename = "cull-mode", default)]
        pub cull_mode: CullMode,

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
        pub disable_depth_test: bool,
        pub cull_mode: CullMode,
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
                        disable_depth_test: declaration.disable_depth_test,
                        cull_mode: declaration.cull_mode,
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

slotmap::new_key_type! { pub struct CubemapId; }

#[derive(Debug, Clone)]
pub struct Cubemap {
        pub faces: [Image; 6],
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

                let images_by_index = Self::load_images(gltf, &doc, image_data, &mut assets.images)?;
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

        fn load_images(
                gltf_path: &Path,
                doc: &gltf::Document,
                image_data: Vec<gltf::image::Data>,
                out_images: &mut ObservableSlotMap<ImageId, Image, AssetManagerEvent>,
        ) -> Result<Vec<ImageId>, GLTFImportError> {
                image_data
                        .into_iter()
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

                                let image_id = out_images.insert(Image {
                                        pixels: image.pixels,
                                        width: image.width,
                                        height: image.height,
                                        format: image.format,
                                });

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
                images_by_index: &Vec<ImageId>,
                samplers_by_index: &Vec<SamplerId>,
                out_textures: &mut ObservableSlotMap<TextureId, Texture, AssetManagerEvent>,
        ) -> Vec<TextureId> {
                doc.textures()
                        .map(|t| {
                                let tex_id = out_textures.insert(Texture {
                                        name: t.name().map(String::from),
                                        image: images_by_index[t.source().index()],
                                        sampler: t
                                                .sampler()
                                                .index()
                                                .map_or(SamplerId::default(), |i| samplers_by_index[i]),
                                });

                                tex_id
                        })
                        .collect()
        }

        fn load_materials(
                doc: &gltf::Document,
                textures_by_index: &Vec<TextureId>,
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
                                let normal_texture = m.normal_texture().map(|t| textures_by_index[t.texture().index()]);
                                let occlusion_texture =
                                        m.occlusion_texture().map(|t| textures_by_index[t.texture().index()]);
                                let emissive_texture =
                                        m.emissive_texture().map(|t| textures_by_index[t.texture().index()]);
                                let emissive_factor = Vec3::from_slice(&m.emissive_factor());

                                let mat_id = out_materials.insert(Material {
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
                                });

                                mat_id
                        })
                        .collect()
        }

        fn load_meshes(
                doc: &gltf::Document,
                buffer_data: Vec<gltf::buffer::Data>,
                materials_by_index: &Vec<MaterialId>,
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
                out_models: &mut ObservableSlotMap<ModelId, Model, AssetManagerEvent>,
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
                models_by_index: &Vec<ModelId>,
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

                        model.meshes.iter_mut().for_each(|mid| *mid = new_mesh_ids[*mid]);
                        model.children.iter_mut().for_each(|mid| *mid = new_model_ids[*mid]);
                }

                for (_, &new_key) in &new_mesh_ids {
                        let mesh = &mut self.meshes[new_key];

                        mesh.material = if mesh.material != default() {
                                new_material_ids[mesh.material]
                        } else {
                                default_material
                        }
                }

                for (_, &new_key) in &new_material_ids {
                        let default_base_color_texture = self.materials[default_material].base_color_texture;
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

        shader_resources: HashMap<ShaderResourceId, ShaderResource>,
        shader_names: HashMap<String, ShaderId>,

        pub skybox_model: ModelId,

        default_sampler: SamplerId,
        default_material: MaterialId,
        default_shader: ShaderId,
}

impl AssetManager {
        pub fn new() -> AnyResult<(Self, Receiver<AssetManagerEvent>)> {
                let (mut assets, event_rx) = AssetStorage::new();

                let default_sampler = assets.samplers.insert(Sampler {
                        name: Some("default-sampler".into()),
                        mag_filter: MagFilter::Linear,
                        min_filter: MinFilter::LinearMipmapLinear,
                        wrap_s: WrappingMode::Repeat,
                        wrap_t: WrappingMode::Repeat,
                });

                let default_diffuse_image = assets.images.insert(Image {
                        pixels: vec![u8::MAX; 4],
                        width: 1,
                        height: 1,
                        format: Format::R8G8B8A8,
                });

                let default_diffuse_texture = assets.textures.insert(Texture {
                        name: Some("default-diffuse-texture".into()),
                        image: default_diffuse_image,
                        sampler: default_sampler,
                });

                let default_specular_image = assets.images.insert(Image {
                        pixels: vec![u8::MAX; 4],
                        width: 1,
                        height: 1,
                        format: Format::R8G8B8A8,
                });

                let default_specular_texture = assets.textures.insert(Texture {
                        name: Some("default-specular-texture".into()),
                        image: default_specular_image,
                        sampler: default_sampler,
                });

                let default_shader = assets.shaders.insert(Shader::from_yaml(Path::new(
                        "res/shader/basic_shader/basic_shader.yaml",
                ))?);

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
                        normal_texture: None,
                        occlusion_texture: None,
                        emissive_texture: None,
                        emissive_factor: Vec3::splat(0.0),
                });

                let skybox_shader = Shader::from_yaml(Path::new("res/shader/skybox_shader/skybox_shader.yaml"))?;
                let skybox_shader = assets.shaders.insert(skybox_shader);

                let (cube_bundle, cube_model_name) = AssetBundle::from_gltf(Path::new("res/model/cube/cube.gltf"))?;
                assets.extend(cube_bundle.assets, default_sampler, default_material, default_shader);

                let cube_model = &assets.models[assets.models[assets.named_models[&cube_model_name]].children[0]];
                let cube_mesh = &assets.meshes[cube_model.meshes[0]];
                let cube_material = &assets.materials[cube_mesh.material];

                let mut skybox_material = cube_material.clone();
                skybox_material.shader = skybox_shader;
                let skybox_material = assets.materials.insert(skybox_material);

                let mut skybox_mesh = cube_mesh.clone();
                skybox_mesh.material = skybox_material;
                let skybox_mesh = assets.meshes.insert(skybox_mesh);

                let mut skybox_model = cube_model.clone();
                skybox_model.meshes[0] = skybox_mesh;
                let skybox_model = assets.models.insert(skybox_model);

                Ok((
                        Self {
                                assets,

                                shader_resources: HashMap::new(),
                                shader_names: HashMap::new(),

                                skybox_model,

                                default_sampler,
                                default_material,
                                default_shader,
                        },
                        event_rx,
                ))
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
                let shader_id = self.assets.shaders.insert(shader);

                debug!("Loaded shader with name: {}", shader_name);

                self.shader_names.insert(shader_name, shader_id);

                Ok(shader_id)
        }

        pub fn insert_cubemap(&mut self, cubemap: Cubemap) -> CubemapId {
                self.assets.cubemaps.insert(cubemap)
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
        pub fn shader_resources(&self) -> &HashMap<ShaderResourceId, ShaderResource> {
                &self.shader_resources
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
        let data = unsafe { buffer_data.0.as_ptr().offset(byte_offset as isize) };

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
                self.event_tx.send(SlotMapEvent::Inserted(k).into());
                self.event_tx.send(SlotMapEvent::Changed(k).into());
                k
        }

        #[allow(dead_code)]
        pub fn remove(&mut self, k: K) -> Option<V> {
                let v = self.inner.remove(k);
                self.event_tx.send(SlotMapEvent::Removed(k).into());
                v
        }

        #[allow(dead_code)]
        pub fn get(&self, k: K) -> Option<&V> {
                self.inner.get(k)
        }

        #[allow(dead_code)]
        pub fn get_mut(&mut self, k: K) -> Option<&mut V> {
                self.event_tx.send(SlotMapEvent::Changed(k).into());
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
                self.event_tx.send(SlotMapEvent::Changed(index).into());
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
