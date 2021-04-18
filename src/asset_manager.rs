use std::{
        collections::HashMap,
        path::{Path, PathBuf},
};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::{
        my_glm::*,
        vec_map::{VecMap, VecMapKey},
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

#[derive(Debug)]
pub struct Model {
        pub name: Option<String>,
        pub transform: Mat4,
        pub mesh: Option<MeshID>,
        pub children: Vec<ModelID>,
}

#[derive(Debug)]
pub struct Mesh {
        pub name: Option<String>,
        pub primitives: Vec<Primitive>,
}

#[derive(Debug)]
pub struct Primitive {
        pub positions: BufferViewID,
        pub tex_coords: BufferViewID,
        pub normals: BufferViewID,
        pub tangents: BufferViewID,
        pub indices: BufferViewID,
        pub material: MaterialID,
        pub bounding_box: BoundingBox,
}

#[derive(Debug)]
pub struct BufferView {
        pub buffer_id: BufferID,
        pub byte_length: usize,
        pub byte_offset: usize,
        pub component_type: ComponentType,
        pub data_type: DataType,
        pub element_count: usize,
}

#[derive(Debug)]
pub struct Buffer {
        pub bytes: Vec<u8>,
        pub byte_length: usize,
}

#[derive(Debug)]
pub struct Material {
        pub name: Option<String>,

        pub base_color_factor: Vec4,
        pub metallic_factor: f32,
        pub roughness_factor: f32,

        pub base_color_texture: Option<TextureID>,
        pub metallic_roughness_texture: Option<TextureID>,
        pub normal_texture: Option<TextureID>,
        pub occlusion_texture: Option<TextureID>,
        pub emissive_texture: Option<TextureID>,
}

#[derive(Debug)]
pub struct Texture {
        pub name: Option<String>,
        pub image: ImageID,
        pub sampler: SamplerID,
}

pub type ImageFormat = gltf::image::Format;
#[derive(Debug)]
pub struct Image {
        //name: Option<String>,
        pub pixels: Vec<u8>,
        pub width: u32,
        pub height: u32,
        pub format: ImageFormat,
}

pub type MagFilter = gltf::texture::MagFilter;
pub type MinFilter = gltf::texture::MinFilter;
pub type WrappingMode = gltf::texture::WrappingMode;

#[derive(Debug)]
pub struct Sampler {
        pub name: Option<String>,
        pub mag_filter: MagFilter,
        pub min_filter: MinFilter,
        pub wrap_s: WrappingMode,
        pub wrap_t: WrappingMode,
}

new_vec_map_keys!(
        ModelID,
        MeshID,
        BufferID,
        BufferViewID,
        MaterialID,
        TextureID,
        ImageID,
        SamplerID
);

#[derive(Debug)]
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

#[derive(Debug)]
#[allow(dead_code)]
pub enum GLTFImportError {
        ImageSourceNotUri,
        ImageSourceUriNotRelative,
        ImageFormatNotSupported,
        AccessorMissingBufferView,
        MeshMissingPrimitives,
        MeshMissingPositions,
        MeshMissingTexCoords,
        MeshMissingNormals,
        MeshMissingTangents,
        MeshMissingIndices,
        GLTFCrateError(gltf::Error),
}

impl std::fmt::Display for GLTFImportError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self)
        }
}

impl std::error::Error for GLTFImportError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                        GLTFImportError::GLTFCrateError(e) => Some(e),
                        _ => None,
                }
        }
}

pub struct AssetManager {
        buffers: VecMap<BufferID, Buffer>,
        buffer_views: VecMap<BufferViewID, BufferView>,
        images: VecMap<ImageID, Image>,
        image_path_map: HashMap<PathBuf, ImageID>,
        samplers: VecMap<SamplerID, Sampler>,
        textures: VecMap<TextureID, Texture>,
        materials: VecMap<MaterialID, Material>,
        meshes: VecMap<MeshID, Mesh>,
        models: VecMap<ModelID, Model>,
        root_models: Vec<ModelID>,

        default_sampler: SamplerID,
        default_material: MaterialID,
}

impl AssetManager {
        pub fn new(default_sampler: Sampler, default_material: Material) -> Self {
                let mut samplers = VecMap::new();
                let default_sampler = samplers.insert(default_sampler);

                let mut materials = VecMap::new();
                let default_material = materials.insert(default_material);

                Self {
                        buffers: VecMap::new(),
                        buffer_views: VecMap::new(),
                        images: VecMap::new(),
                        image_path_map: HashMap::new(),
                        samplers,
                        textures: VecMap::new(),
                        materials: VecMap::new(),
                        meshes: VecMap::new(),
                        models: VecMap::new(),
                        root_models: Vec::new(),
                        default_sampler,
                        default_material,
                }
        }

        pub fn import_gltf_file(&mut self, gltf_path: &Path) -> Result<Vec<ModelID>, GLTFImportError> {
                timer!("Loaded model in ");

                let (doc, buffers, images) = gltf::import(gltf_path).map_err(|e| GLTFImportError::GLTFCrateError(e))?;

                let buffer_ids = Self::load_buffers(buffers, &mut self.buffers);
                let buffer_view_ids = Self::load_buffer_views(&doc, &buffer_ids, &mut self.buffer_views)?;
                let image_ids = Self::load_images(gltf_path, &doc, images, &mut self.image_path_map, &mut self.images)?;
                let sampler_ids = Self::load_samplers(&doc, self.default_sampler, &mut self.samplers);
                let texture_ids =
                        Self::load_textures(&doc, &image_ids, &sampler_ids, self.default_sampler, &mut self.textures);
                let material_ids = Self::load_materials(&doc, &texture_ids, &mut self.materials);
                let meshes_ids = Self::load_meshes(
                        &doc,
                        &buffer_view_ids,
                        &material_ids,
                        self.default_material,
                        &mut self.meshes,
                )?;
                let model_ids = Self::load_models(&doc, &meshes_ids, &mut self.models);
                let root_model_ids = Self::load_root_models(&doc, &model_ids, &mut self.models, &mut self.root_models);

                info!("Imported #{} models", root_model_ids.len());

                Ok(root_model_ids)
        }

        #[allow(dead_code)]
        pub fn buffers(&self) -> &VecMap<BufferID, Buffer> {
                &self.buffers
        }

        #[allow(dead_code)]
        pub fn buffer_views(&self) -> &VecMap<BufferViewID, BufferView> {
                &self.buffer_views
        }

        #[allow(dead_code)]
        pub fn images(&self) -> &VecMap<ImageID, Image> {
                &self.images
        }

        #[allow(dead_code)]
        pub fn samplers(&self) -> &VecMap<SamplerID, Sampler> {
                &self.samplers
        }

        #[allow(dead_code)]
        pub fn textures(&self) -> &VecMap<TextureID, Texture> {
                &self.textures
        }

        #[allow(dead_code)]
        pub fn materials(&self) -> &VecMap<MaterialID, Material> {
                &self.materials
        }

        #[allow(dead_code)]
        pub fn meshes(&self) -> &VecMap<MeshID, Mesh> {
                &self.meshes
        }

        #[allow(dead_code)]
        pub fn models(&self) -> &VecMap<ModelID, Model> {
                &self.models
        }

        #[allow(dead_code)]
        pub fn root_models(&self) -> &Vec<ModelID> {
                &self.root_models
        }


        fn load_buffers(
                buffers_data: Vec<gltf::buffer::Data>,
                out_buffers: &mut VecMap<BufferID, Buffer>,
        ) -> Vec<BufferID> {
                buffers_data
                        .into_iter()
                        .map(|buffer| {
                                let buffer = buffer.0;
                                let buffer_id = out_buffers.insert(Buffer {
                                        byte_length: buffer.len(),
                                        bytes: buffer,
                                });

                                buffer_id
                        })
                        .collect()
        }

        fn load_buffer_views(
                doc: &gltf::Document,
                buffer_ids: &Vec<BufferID>,
                out_buffer_views: &mut VecMap<BufferViewID, BufferView>,
        ) -> Result<Vec<BufferViewID>, GLTFImportError> {
                doc.accessors()
                        .map(|a| {
                                let bview = match a.view() {
                                        Some(bview) => bview,
                                        None => {
                                                error!("Accessor is missing buffer view index!");
                                                return Err(GLTFImportError::AccessorMissingBufferView);
                                        }
                                };

                                let buffer_view_id = out_buffer_views.insert(BufferView {
                                        buffer_id: buffer_ids[bview.buffer().index()],
                                        byte_length: bview.length(),
                                        byte_offset: bview.offset() + a.offset(),
                                        component_type: a.data_type(),
                                        data_type: a.dimensions(),
                                        element_count: a.count(),
                                });

                                Ok(buffer_view_id)
                        })
                        .collect()
        }

        fn load_images(
                gltf_path: &Path,
                doc: &gltf::Document,
                images: Vec<gltf::image::Data>,
                image_path_map: &mut HashMap<PathBuf, ImageID>,
                out_images: &mut VecMap<ImageID, Image>,
        ) -> Result<Vec<ImageID>, GLTFImportError> {
                images.into_iter()
                        .zip(doc.images())
                        .filter_map(|(image, json_image)| {
                                if !Self::is_image_format_supported(image.format) {
                                        return Some(Err(GLTFImportError::ImageFormatNotSupported));
                                }

                                // Image path relative to working directory
                                let image_relative_path = match json_image.source() {
                                        gltf::image::Source::Uri { uri, .. } => {
                                                if uri.contains(":") {
                                                        error!("Trying to import image with non relative uri!");
                                                        return Some(Err(GLTFImportError::ImageSourceUriNotRelative));
                                                }

                                                gltf_path.join(uri)
                                        }
                                        _ => {
                                                error!("Image source is not an uri!");
                                                return Some(Err(GLTFImportError::ImageSourceNotUri));
                                        }
                                };

                                let image_id = match image_path_map.get(&image_relative_path) {
                                        Some(&image_id) => image_id,
                                        None => {
                                                let image_id = out_images.insert(Image {
                                                        pixels: image.pixels,
                                                        width: image.width,
                                                        height: image.height,
                                                        format: image.format,
                                                });

                                                image_path_map.insert(image_relative_path, image_id);

                                                image_id
                                        }
                                };

                                Some(Ok(image_id))
                        })
                        .collect()
        }

        fn is_image_format_supported(format: gltf::image::Format) -> bool {
                type Format = gltf::image::Format;

                match format {
                        Format::R8G8B8A8 => true,
                        _ => false,
                }
        }

        fn load_samplers(
                doc: &gltf::Document,
                default_sampler: SamplerID,
                out_samplers: &mut VecMap<SamplerID, Sampler>,
        ) -> Vec<SamplerID> {
                doc.samplers()
                        .map(|s| {
                                let sampler_id = out_samplers.insert(Sampler {
                                        name: s.name().map(String::from),
                                        mag_filter: s.mag_filter().unwrap_or(out_samplers[default_sampler].mag_filter),
                                        min_filter: s.min_filter().unwrap_or(out_samplers[default_sampler].min_filter),
                                        wrap_s: s.wrap_s(),
                                        wrap_t: s.wrap_t(),
                                });

                                sampler_id
                        })
                        .collect()
        }

        fn load_textures(
                doc: &gltf::Document,
                image_ids: &Vec<ImageID>,
                sampler_ids: &Vec<SamplerID>,
                default_sampler: SamplerID,
                out_textures: &mut VecMap<TextureID, Texture>,
        ) -> Vec<TextureID> {
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
                texture_ids: &Vec<TextureID>,
                out_materials: &mut VecMap<MaterialID, Material>,
        ) -> Vec<MaterialID> {
                doc.materials()
                        .map(|m| {
                                // TODO: handle textures better

                                let mat_id = out_materials.insert(Material {
                                        name: m.name().map(String::from),
                                        base_color_factor: m.pbr_metallic_roughness().base_color_factor().into(),
                                        metallic_factor: m.pbr_metallic_roughness().metallic_factor(),
                                        roughness_factor: m.pbr_metallic_roughness().roughness_factor(),
                                        base_color_texture: m
                                                .pbr_metallic_roughness()
                                                .base_color_texture()
                                                .map(|t| texture_ids[t.texture().index()]),
                                        metallic_roughness_texture: m
                                                .pbr_metallic_roughness()
                                                .metallic_roughness_texture()
                                                .map(|t| texture_ids[t.texture().index()]),
                                        normal_texture: m.normal_texture().map(|t| texture_ids[t.texture().index()]),
                                        occlusion_texture: m
                                                .occlusion_texture()
                                                .map(|t| texture_ids[t.texture().index()]),
                                        emissive_texture: m
                                                .emissive_texture()
                                                .map(|t| texture_ids[t.texture().index()]),
                                });

                                mat_id
                        })
                        .collect()
        }

        fn load_meshes(
                doc: &gltf::Document,
                buffer_view_ids: &Vec<BufferViewID>,
                material_ids: &Vec<MaterialID>,
                default_material: MaterialID,
                out_meshes: &mut VecMap<MeshID, Mesh>,
        ) -> Result<Vec<MeshID>, GLTFImportError> {
                doc.meshes()
                        .map(|m| {
                                let primitives = m
                                        .primitives()
                                        .map(|p| {
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

                                                let material = p
                                                        .material()
                                                        .index()
                                                        .map_or(default_material, |i| material_ids[i]);

                                                Ok(Primitive {
                                                        positions,
                                                        tex_coords,
                                                        normals,
                                                        tangents,
                                                        indices,
                                                        material,
                                                        bounding_box: BoundingBox::from(&p.bounding_box()),
                                                })
                                        })
                                        .collect::<Result<Vec<Primitive>, GLTFImportError>>()?;

                                let mesh = Mesh {
                                        name: m.name().map(String::from),
                                        primitives,
                                };

                                let mesh_id = out_meshes.insert(mesh);

                                Ok(mesh_id)
                        })
                        .collect()
        }

        fn load_models(
                doc: &gltf::Document,
                meshes_ids: &Vec<MeshID>,
                out_models: &mut VecMap<ModelID, Model>,
        ) -> Vec<ModelID> {
                let mut model_ids = Vec::new();

                for n in doc.nodes() {
                        let mesh = n.mesh().map(|m| meshes_ids[m.index()]);

                        let children = n.children().map(|n| model_ids[n.index()]).collect();

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
                                        let t = Mat4::new_translation(&Vec3::from_column_slice(translation));

                                        let r = UnitQuat::new_unchecked(Quat::new(
                                                rotation[3],
                                                rotation[0],
                                                rotation[1],
                                                rotation[2],
                                        ));

                                        let s = Mat4::new_nonuniform_scaling(&Vec3::from_column_slice(scale));

                                        t * r.to_homogeneous() * s
                                }
                        };

                        let model_id = out_models.insert(Model {
                                name: n.name().map(String::from),
                                transform,
                                mesh,
                                children,
                        });

                        model_ids.push(model_id);
                }

                model_ids
        }

        fn load_root_models(
                doc: &gltf::Document,
                model_ids: &Vec<ModelID>,
                out_models: &mut VecMap<ModelID, Model>,
                out_root_models: &mut Vec<ModelID>,
        ) -> Vec<ModelID> {
                doc.scenes()
                        .map(|s| {
                                let children = s.nodes().map(|n| model_ids[n.index()]).collect();

                                let model = Model {
                                        name: s.name().map(String::from),
                                        transform: Mat4::identity(),
                                        mesh: None,
                                        children,
                                };
                                let model_id = out_models.insert(model);

                                out_root_models.push(model_id);
                                model_id
                        })
                        .collect()
        }

        /* fn create_model_from_node_recursively(
                n: &gltf::Node,
                buffer_view_ids: &Vec<BufferViewID>,
                material_ids: &Vec<MaterialID>,
                default_material: MaterialID,
                out_models: &mut VecMap<ModelID, Model>,
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
                                let t = Mat4::new_translation(&Vec3::from_column_slice(translation));

                                let r = UnitQuat::new_unchecked(Quat::new(
                                        rotation[3],
                                        rotation[0],
                                        rotation[1],
                                        rotation[2],
                                ));

                                let s = Mat4::new_nonuniform_scaling(&Vec3::from_column_slice(scale));

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
