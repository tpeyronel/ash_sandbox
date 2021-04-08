use std::{
        collections::HashMap,
        path::{Path, PathBuf},
};

use log::{error, info};

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

pub struct Model {
        pub name: Option<String>,
        pub meshes: Vec<Mesh>,
        pub children: Vec<ModelID>,
}

pub struct Mesh {
        pub positions: BufferViewID,
        pub tex_coords: BufferViewID,
        pub normals: BufferViewID,
        pub tangents: BufferViewID,
        pub indices: BufferViewID,
        pub material_id: MaterialID,
        pub bounding_box: BoundingBox,
}

pub struct BufferView {
        pub buffer_id: BufferID,
        pub byte_length: usize,
        pub byte_offset: usize,
        pub component_type: ComponentType,
        pub data_type: DataType,
        pub element_count: usize,
}

pub struct Buffer {
        pub bytes: Vec<u8>,
        pub byte_length: usize,
}

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

pub struct Texture {
        pub name: Option<String>,
        pub image: ImageID,
        pub sampler: SamplerID,
}

pub type ImageFormat = gltf::image::Format;
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

pub struct Sampler {
        pub name: Option<String>,
        pub mag_filter: MagFilter,
        pub min_filter: MinFilter,
        pub wrap_s: WrappingMode,
        pub wrap_t: WrappingMode,
}

new_vec_map_keys!(
        ModelID, /*, MeshID*/
        BufferID,
        BufferViewID,
        MaterialID,
        TextureID,
        ImageID,
        SamplerID
);

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
pub enum GLTFImportError {
        ImageSourceNotUri,
        ImageSourceUriNotRelative,
        ImageFormatNotSupported,
        AccessorMissingBufferView,
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
        //meshes: VecMap<MeshID, Mesh>,
        models: VecMap<ModelID, Model>,

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
                        //meshes: VecMap::new(),
                        models: VecMap::new(),
                        default_sampler,
                        default_material,
                }
        }

        pub fn import_gltf_file(&mut self, gltf_path: &Path) -> Result<Vec<ModelID>, GLTFImportError> {
                let (doc, buffers, images) =
                        gltf::import(gltf_path).map_err(|e| GLTFImportError::GLTFCrateError(e))?;

                let buffer_ids = self.load_buffers(buffers);
                let buffer_view_ids = self.load_buffer_views(&doc, &buffer_ids)?;
                let image_ids = self.load_images(gltf_path, &doc, images)?;
                let sampler_ids = self.load_samplers(&doc);
                let texture_ids = self.load_textures(&doc, &image_ids, &sampler_ids);
                let material_ids = self.load_materials(&doc, &texture_ids);
                let model_ids = self.load_models(&doc, &buffer_view_ids, &material_ids)?;

                info!("Imported #{} models", model_ids.len());

                Ok(model_ids)
        }

        pub fn buffers(&self) -> &VecMap<BufferID, Buffer> {
                &self.buffers
        }

        pub fn buffer_views(&self) -> &VecMap<BufferViewID, BufferView> {
                &self.buffer_views
        }

        pub fn images(&self) -> &VecMap<ImageID, Image> {
                &self.images
        }

        pub fn samplers(&self) -> &VecMap<SamplerID, Sampler> {
                &self.samplers
        }

        fn load_buffers(&mut self, buffers: Vec<gltf::buffer::Data>) -> Vec<BufferID> {
                buffers.into_iter()
                        .map(|buffer| {
                                let buffer = buffer.0;
                                let buffer_id = self.buffers.insert(Buffer {
                                        byte_length: buffer.len(),
                                        bytes: buffer,
                                });

                                buffer_id
                        })
                        .collect()
        }

        fn load_buffer_views(
                &mut self,
                doc: &gltf::Document,
                buffer_ids: &Vec<BufferID>,
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

                                let buffer_view_id = self.buffer_views.insert(BufferView {
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
                &mut self,
                gltf_path: &Path,
                doc: &gltf::Document,
                images: Vec<gltf::image::Data>,
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

                                if let Some(_) = self.image_path_map.get(&image_relative_path) {
                                        return None;
                                }

                                let image_id = self.images.insert(Image {
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
                        Format::R8G8B8A8 => true,
                        _ => false
                }
        }

        fn load_samplers(&mut self, doc: &gltf::Document) -> Vec<SamplerID> {
                doc.samplers()
                        .map(|s| {
                                let sampler_id = self.samplers.insert(Sampler {
                                        name: s.name().map(String::from),
                                        mag_filter: s
                                                .mag_filter()
                                                .unwrap_or(self.samplers[self.default_sampler].mag_filter),
                                        min_filter: s
                                                .min_filter()
                                                .unwrap_or(self.samplers[self.default_sampler].min_filter),
                                        wrap_s: s.wrap_s(),
                                        wrap_t: s.wrap_t(),
                                });

                                sampler_id
                        })
                        .collect()
        }

        fn load_textures(
                &mut self,
                doc: &gltf::Document,
                image_ids: &Vec<ImageID>,
                sampler_ids: &Vec<SamplerID>,
        ) -> Vec<TextureID> {
                doc.textures()
                        .map(|t| {
                                let tex_id = self.textures.insert(Texture {
                                        name: t.name().map(String::from),
                                        image: image_ids[t.source().index()],
                                        sampler: t.sampler().index().map_or(self.default_sampler, |i| sampler_ids[i]),
                                });

                                tex_id
                        })
                        .collect()
        }

        fn load_materials(&mut self, doc: &gltf::Document, texture_ids: &Vec<TextureID>) -> Vec<MaterialID> {
                doc.materials()
                        .map(|m| {
                                // TODO: handle textures better

                                let mat_id = self.materials.insert(Material {
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

        fn load_models(
                &mut self,
                doc: &gltf::Document,
                buffer_view_ids: &Vec<BufferViewID>,
                material_ids: &Vec<MaterialID>,
        ) -> Result<Vec<ModelID>, GLTFImportError> {
                doc.nodes()
                        .filter_map(|n| {
                                Self::create_model_from_node_recursively(self, &n, buffer_view_ids, material_ids)
                        })
                        .collect()
        }

        fn create_model_from_node_recursively(
                &mut self,
                n: &gltf::Node,
                buffer_view_ids: &Vec<BufferViewID>,
                material_ids: &Vec<MaterialID>,
        ) -> Option<Result<ModelID, GLTFImportError>> {
                let m = match n.mesh() {
                        Some(m) => m,
                        None => return None,
                };

                let meshes = m
                        .primitives()
                        .map(|p| {
                                Ok(Mesh {
                                        positions: match p.get(&gltf::Semantic::Positions) {
                                                Some(positions) => buffer_view_ids[positions.index()],
                                                None => return Err(GLTFImportError::MeshMissingPositions),
                                        },
                                        tex_coords: match p.get(&gltf::Semantic::TexCoords(0)) {
                                                Some(tex_coords) => buffer_view_ids[tex_coords.index()],
                                                None => return Err(GLTFImportError::MeshMissingTexCoords),
                                        },
                                        normals: match p.get(&gltf::Semantic::Normals) {
                                                Some(normals) => buffer_view_ids[normals.index()],
                                                None => return Err(GLTFImportError::MeshMissingNormals),
                                        },
                                        tangents: match p.get(&gltf::Semantic::Tangents) {
                                                Some(tangents) => buffer_view_ids[tangents.index()],
                                                None => return Err(GLTFImportError::MeshMissingTangents),
                                        },
                                        indices: match p.indices() {
                                                Some(indices) => buffer_view_ids[indices.index()],
                                                None => return Err(GLTFImportError::MeshMissingIndices),
                                        },
                                        material_id: p
                                                .material()
                                                .index()
                                                .map_or(self.default_material, |i| material_ids[i]),
                                        bounding_box: BoundingBox::from(&p.bounding_box()),
                                })
                        })
                        .collect::<Result<Vec<Mesh>, GLTFImportError>>();

                let meshes = match meshes {
                        Ok(meshes) => meshes,
                        Err(e) => return Some(Err(e)),
                };

                let children = n
                        .children()
                        .filter_map(|n| {
                                Self::create_model_from_node_recursively(self, &n, buffer_view_ids, material_ids)
                        })
                        .collect::<Result<Vec<ModelID>, GLTFImportError>>();

                let children = match children {
                        Ok(children) => children,
                        Err(e) => return Some(Err(e)),
                };

                let model_id = self.models.insert(Model {
                        name: n.name().map(String::from),
                        meshes,
                        children,
                });

                Some(Ok(model_id))
        }
}
