use std::{
        ffi::CString,
        path::{Path, PathBuf},
        rc::Rc,
};

use ash::{
        prelude::VkResult,
        vk::{self, BufferUsageFlags},
};
use crossbeam_channel::Receiver;
use enum_map::EnumMap;
use hashbrown::HashMap;
#[allow(unused_imports)]
use log::{debug, error, info, trace};
use slotmap::SecondaryMap;

use crate::{
        asset_manager::{
                AssetManager, AssetManagerEvent, Cubemap, CubemapId, CullMode, Image, ImageId, IndicesVec, MagFilter,
                MaterialId, MeshId, MinFilter, SamplerId, Shader, ShaderId, ShaderModule, ShaderRenderStage,
                ShaderResourceData, WrappingMode,
        },
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE, PREFILTER_MAP_SIZE},
        hashmap::GetOrInsert,
        my_glm::{Mat4, Vec2, Vec3, Vec4},
        renderer::PrefilterParams,
        shader_preprocessor::{PreprocessedShaderStage, ShaderStageSourceBuilder},
        shader_resource::{ShaderResourceId, ShaderResourceProvider, ShaderResourceType},
        shader_resource_registry::ShaderResourceRegistry,
        shader_resources::{
                SHADER_RESOURCE_ENVIRONMENT_MAP, SHADER_RESOURCE_EQUIRECTANGULAR_MAP, SHADER_RESOURCE_PREFILTER_PARAMS,
        },
        util::{RefIntoBytesSlice, RefIntoSlice},
        vk::{
                vk_buffer::{BufferData, VkBuffer, VkImmutableBufferCreateInfo},
                vk_command_buffer::VkReusableCommandBuffer,
                vk_image::{MipLevels, VkImage},
                vk_wrapper::{VkDevice, VkImageView, VkPhysicalDevice, VkSampler},
        },
        AnyResult,
};

use super::{
        vk_buffer::VkDynamicUniformBuffer,
        vk_context::{VkContext, ENABLE_VALIDATION_LAYERS},
        vk_descriptor_set_allocator::VkDescriptorSetAllocator,
        vk_image::{
                GenerateMipmapsInfo, TransitionImageLayoutInfo, VkImageCreateFromImageInfo, VkImageCubemapCreateInfo,
        },
        vk_util::{vk_format_from_image_format_and_color_space, BytesPerPixel},
        vk_wrapper::{
                VkDebugUtils, VkFramebuffer, VkInstance, VkObject, VkPipeline, VkPipelineLayout, VkShaderModule,
                VmaAllocator,
        },
};

pub struct VkMesh {
        pub positions: VkBuffer,
        pub tex_coords: VkBuffer,
        pub normals: VkBuffer,
        pub tangents: VkBuffer,
        pub indices: VkIndexBuffer,
}

pub struct VkIndexBuffer {
        pub buffer: VkBuffer,
        pub index_type: vk::IndexType,
        pub index_count: u32,
}

pub struct VkModelImage {
        pub image: VkImage,
        pub image_view: VkImageView,
}

pub struct VkMaterial {
        pub dst_sets: Vec<vk::DescriptorSet>,                  // One per concurrent frame
        pub buffers: HashMap<ShaderResourceId, Vec<VkBuffer>>, // One buffer per concurrent frame
}

pub struct VkShader {
        pub vert_module: VkShaderModule,
        pub frag_module: Option<VkShaderModule>,
        pub vertex_input_bindings: Vec<vk::VertexInputBindingDescription>,
        pub vertex_input_attributes: Vec<vk::VertexInputAttributeDescription>,
        pub shader_resource_bindings: HashMap<ShaderResourceId, VkShaderResourceBindingDescription>,

        pub world_dst_set: Vec<vk::DescriptorSet>, // One per concurrent frame
        pub mesh_dst_set: Vec<vk::DescriptorSet>,  // One per concurrent frame

        pub dst_set_layouts: EnumMap<VkDescriptorSetIndex, vk::DescriptorSetLayout>,

        pub graphics_pipeline_layout: VkPipelineLayout,
        pub graphics_pipeline: VkPipeline,
}

impl VkShader {
        fn destroy(&self, device: &VkDevice) {
                unsafe {
                        self.graphics_pipeline.destroy();
                        self.graphics_pipeline_layout.destroy();

                        for (_, &dst_set_layout) in &self.dst_set_layouts {
                                device.destroy_descriptor_set_layout(dst_set_layout, None);
                        }

                        self.frag_module.as_ref().map(|x| x.destroy());
                        self.vert_module.destroy();
                }
        }
}

pub struct VkShaderResourceBindingDescription {
        pub set: VkDescriptorSetIndex,
        pub binding: u32,
        pub descriptor_type: vk::DescriptorType,
}

pub struct VkCubemap {
        pub environment_image: VkImage, // cubemap
        pub environment_image_view: VkImageView,
        pub environment_sampler: VkSampler,
        pub irradiance_image: VkImage, // cubemap
        pub irradiance_image_view: VkImageView,
        pub irradiance_sampler: VkSampler,
        pub prefiltered_image: VkImage, // cubemap
        pub prefiltered_image_view: VkImageView,
        pub prefiltered_sampler: VkSampler,
}

pub struct VkAssetManager {
        instance: Rc<VkInstance>,
        pdevice: Rc<VkPhysicalDevice>,
        device: Rc<VkDevice>,
        debug_utils: Option<Rc<VkDebugUtils>>,
        allocator: Rc<VmaAllocator>,
        transfer_queue: vk::Queue,
        // dst set allocator that is never reset. Used for permanent descriptor sets.
        dst_set_allocator: VkDescriptorSetAllocator,
        // dst set allocators (one per concurrent frame) that are reset every frame. Used for temporary descriptor sets.
        frame_dst_set_allocators: Vec<VkDescriptorSetAllocator>,
        cmd_buffer: VkReusableCommandBuffer,
        concurrent_frames: usize,

        swapchain_samples: vk::SampleCountFlags,

        render_passes: EnumMap<ShaderRenderStage, vk::RenderPass>,

        pub meshes: SecondaryMap<MeshId, VkMesh>,
        pub images: SecondaryMap<ImageId, VkModelImage>,
        pub samplers: SecondaryMap<SamplerId, VkSampler>,
        pub materials: SecondaryMap<MaterialId, VkMaterial>,
        pub shaders: SecondaryMap<ShaderId, VkShader>,
        pub cubemaps: SecondaryMap<CubemapId, VkCubemap>,

        pub shader_resources: HashMap<ShaderResourceId, VkShaderResource>,

        pub shader_resource_buffers: HashMap<ShaderResourceId, Vec<VkBuffer>>, // One buffer per frame
        pub shader_resource_dynamic_buffers: HashMap<ShaderResourceId, Vec<VkDynamicUniformBuffer>>, // One buffer per frame
}

impl VkAssetManager {
        pub fn new(
                vk_context: &mut VkContext,
                swapchain_samples: vk::SampleCountFlags,
                render_passes: EnumMap<ShaderRenderStage, vk::RenderPass>,
                concurrent_frames: usize,
        ) -> AnyResult<Self> {
                assert!(concurrent_frames > 0, "Frames in flight must be greater to zero");

                let dst_set_allocator = VkDescriptorSetAllocator::new(Rc::clone(&vk_context.device))?;
                let mut frame_dst_set_allocators = vec![];
                for _ in 0..concurrent_frames {
                        frame_dst_set_allocators.push(VkDescriptorSetAllocator::new(Rc::clone(&vk_context.device))?);
                }
                let cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&vk_context.device), Rc::clone(&vk_context.cmd_pool))?;

                Ok(Self {
                        instance: Rc::clone(&vk_context.instance),
                        pdevice: Rc::clone(&vk_context.pdevice),
                        device: Rc::clone(&vk_context.device),
                        debug_utils: vk_context.debug_utils.as_ref().map(|d| Rc::clone(d)),
                        allocator: Rc::clone(&vk_context.allocator),
                        transfer_queue: vk_context.queues.graphics,
                        dst_set_allocator,
                        frame_dst_set_allocators,
                        cmd_buffer,
                        concurrent_frames,

                        swapchain_samples,
                        render_passes,

                        meshes: SecondaryMap::new(),
                        images: SecondaryMap::new(),
                        samplers: SecondaryMap::new(),
                        materials: SecondaryMap::new(),
                        shaders: SecondaryMap::new(),
                        cubemaps: SecondaryMap::new(),

                        shader_resources: HashMap::new(),
                        shader_resource_buffers: HashMap::new(),
                        shader_resource_dynamic_buffers: HashMap::new(),
                })
        }

        pub fn process_asset_manager_events(
                &mut self,
                asset_manager: &AssetManager,
                asset_manager_event_rx: &Receiver<AssetManagerEvent>,
        ) -> AnyResult<()> {
                for e in asset_manager_event_rx.try_iter() {
                        match e {
                                AssetManagerEvent::MeshChanged(_) => (),
                                AssetManagerEvent::MeshInserted(mesh_id) => {
                                        self.on_mesh_updated(asset_manager, mesh_id)?;
                                },
                                AssetManagerEvent::MeshRemoved(_) => todo!(),
                                AssetManagerEvent::ModelInserted(_) => (),
                                AssetManagerEvent::ModelChanged(_) => (),
                                AssetManagerEvent::ModelRemoved(_) => todo!(),
                                AssetManagerEvent::ImageChanged(_) => (),
                                AssetManagerEvent::ImageInserted(image_id) => {
                                        self.on_image_updated(asset_manager, image_id)?;
                                },
                                AssetManagerEvent::ImageRemoved(_) => todo!(),
                                AssetManagerEvent::SamplerChanged(_) => (),
                                AssetManagerEvent::SamplerInserted(sampler_id) => {
                                        self.on_sampler_updated(asset_manager, sampler_id)?;
                                },
                                AssetManagerEvent::SamplerRemoved(_) => todo!(),
                                AssetManagerEvent::TextureInserted(_) => (),
                                AssetManagerEvent::TextureChanged(_) => (),
                                AssetManagerEvent::TextureRemoved(_) => todo!(),
                                AssetManagerEvent::MaterialChanged(_) => (),
                                AssetManagerEvent::MaterialInserted(material_id) => {
                                        self.on_material_updated(asset_manager, material_id)?;
                                },
                                AssetManagerEvent::MaterialRemoved(_) => todo!(),
                                AssetManagerEvent::ShaderResourceInserted(shader_resource_id) => {
                                        self.on_shader_resource_inserted(asset_manager, shader_resource_id)?;
                                },
                                AssetManagerEvent::ShaderResourceChanged(_) => todo!(),
                                AssetManagerEvent::ShaderResourceRemoved(_) => todo!(),
                                AssetManagerEvent::ShaderChanged(_) => (),
                                AssetManagerEvent::ShaderInserted(shader_id) => {
                                        self.on_shader_updated(asset_manager, shader_id)?;
                                },
                                AssetManagerEvent::ShaderRemoved(_) => todo!(),
                                AssetManagerEvent::CubemapChanged(_) => (),
                                AssetManagerEvent::CubemapInserted(cubemap_id) => {
                                        self.on_cubemap_updated(asset_manager, cubemap_id)?;
                                },
                                AssetManagerEvent::CubemapRemoved(_) => todo!(),
                        }
                }

                Ok(())
        }

        pub fn notify_new_frame(&mut self, framei: usize) -> VkResult<()> {
                unsafe { self.frame_dst_set_allocators[framei].reset_pools()? };

                Ok(())
        }

        pub fn destroy(&mut self) {
                self.shader_resource_dynamic_buffers
                        .drain()
                        .for_each(|(_, mut buffers)| {
                                buffers.drain(..).for_each(|b| unsafe { b.destroy() });
                        });

                self.shader_resource_buffers.drain().for_each(|(_, mut buffers)| {
                        buffers.drain(..).for_each(|b| unsafe { b.destroy() });
                });

                self.cubemaps.drain().for_each(|(_, cubemap)| unsafe {
                        cubemap.prefiltered_sampler.destroy();
                        cubemap.prefiltered_image_view.destroy();
                        cubemap.prefiltered_image.destroy();
                        cubemap.irradiance_sampler.destroy();
                        cubemap.irradiance_image_view.destroy();
                        cubemap.irradiance_image.destroy();
                        cubemap.environment_sampler.destroy();
                        cubemap.environment_image_view.destroy();
                        cubemap.environment_image.destroy();
                });

                self.shaders
                        .drain()
                        .for_each(|(_, shader)| shader.destroy(&self.device));

                self.materials.drain().for_each(|(_, mut material)| {
                        material.buffers.drain().for_each(|(_, mut buffers)| {
                                buffers.drain(..).for_each(|b| unsafe { b.destroy() });
                        });
                });

                self.samplers.drain().for_each(|(_, sampler)| unsafe {
                        sampler.destroy();
                });

                self.images.drain().for_each(|(_, image)| unsafe {
                        image.image.destroy();
                        image.image_view.destroy();
                });

                self.meshes.drain().for_each(|(_, mesh)| unsafe {
                        mesh.positions.destroy();
                        mesh.tex_coords.destroy();
                        mesh.normals.destroy();
                        mesh.tangents.destroy();
                        mesh.indices.buffer.destroy();
                });

                unsafe { self.cmd_buffer.destroy() };
                for dst_set_allocator in &mut self.frame_dst_set_allocators {
                        unsafe { dst_set_allocator.destroy() };
                }
                unsafe { self.dst_set_allocator.destroy() };
        }

        fn create_graphics_pipeline_layout(
                device: &Rc<VkDevice>,
                dst_set_layouts: &[vk::DescriptorSetLayout],
                push_constants_size: u32,
        ) -> VkResult<VkPipelineLayout> {
                let push_constant_range = vk::PushConstantRange {
                        stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        offset: 0,
                        size: push_constants_size,
                };

                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder().set_layouts(dst_set_layouts);
                let layout_cinfo = if push_constants_size > 0 {
                        layout_cinfo.push_constant_ranges(std::slice::from_ref(&push_constant_range))
                } else {
                        layout_cinfo
                };

                unsafe { VkPipelineLayout::new(device, &layout_cinfo) }
        }

        fn on_mesh_updated(&mut self, asset_manager: &AssetManager, mesh_id: MeshId) -> AnyResult<()> {
                if self.meshes.contains_key(mesh_id) {
                        // TODO: handle mesh update.
                        todo!();
                } else {
                        self.create_vk_mesh(asset_manager, mesh_id)?;
                }

                Ok(())
        }

        fn on_image_updated(&mut self, asset_manager: &AssetManager, image_id: ImageId) -> AnyResult<()> {
                if self.images.contains_key(image_id) {
                        todo!();
                } else if let Some(vk_image) = self.create_vk_image_from_image(asset_manager, image_id)? {
                        self.images.insert(image_id, vk_image);
                }

                Ok(())
        }

        fn on_sampler_updated(&mut self, asset_manager: &AssetManager, sampler_id: SamplerId) -> AnyResult<()> {
                if self.samplers.contains_key(sampler_id) {
                        todo!();
                } else if let Some(vk_sampler) = self.create_vk_sampler_from_sampler(asset_manager, sampler_id)? {
                        self.samplers.insert(sampler_id, vk_sampler);
                }

                Ok(())
        }

        fn on_material_updated(&mut self, asset_manager: &AssetManager, material_id: MaterialId) -> AnyResult<()> {
                match self.materials.get(material_id) {
                        Some(_vk_material) => {
                                let _material = match asset_manager.get_material(material_id) {
                                        Some(material) => material,
                                        None => return Ok(()),
                                };

                                // self.update_vk_material(asset_manager, material, vk_material.dst_set);
                        },
                        None => {
                                if let Some(vk_material) =
                                        self.create_vk_material_from_material(asset_manager, material_id)?
                                {
                                        self.materials.insert(material_id, vk_material);
                                }
                        },
                }

                Ok(())
        }

        fn on_shader_resource_inserted(
                &mut self,
                asset_manager: &AssetManager,
                shader_resource_id: ShaderResourceId,
        ) -> AnyResult<()> {
                let Some(shader_resource) = asset_manager.shader_resources().get(&shader_resource_id) else {
                        return Ok(());
                };

                let vk_shader_resource_type = match (&shader_resource.resource_type, shader_resource.provider) {
                        (
                                ShaderResourceType::Struct(declaration),
                                ShaderResourceProvider::World | ShaderResourceProvider::Material,
                        ) => {
                                let buffers = (0..self.concurrent_frames)
                                        .map(|_| {
                                                VkBuffer::new_uniform_buffer(
                                                        &self.device,
                                                        Rc::clone(&self.allocator),
                                                        declaration.compute_size() as vk::DeviceSize,
                                                )
                                        })
                                        .collect::<VkResult<Vec<VkBuffer>>>()?;

                                self.shader_resource_buffers.insert(shader_resource_id.clone(), buffers);

                                VkShaderResourceType::UniformBuffer
                        },
                        (ShaderResourceType::Struct(declaration), ShaderResourceProvider::Mesh) => {
                                let buffers = (0..self.concurrent_frames)
                                        .map(|_| {
                                                VkDynamicUniformBuffer::new(
                                                        &self.pdevice,
                                                        &self.device,
                                                        Rc::clone(&self.allocator),
                                                        declaration.compute_size(),
                                                        1024,
                                                )
                                        })
                                        .collect::<AnyResult<Vec<VkDynamicUniformBuffer>>>()?;

                                self.shader_resource_dynamic_buffers
                                        .insert(shader_resource_id.clone(), buffers);

                                VkShaderResourceType::UniformBufferDynamic
                        },
                        (ShaderResourceType::Image2D, ShaderResourceProvider::World) => {
                                VkShaderResourceType::CombinedImageSampler
                        },
                        (ShaderResourceType::Image2D, ShaderResourceProvider::Material) => {
                                VkShaderResourceType::CombinedImageSampler
                        },
                        (ShaderResourceType::Image2D, ShaderResourceProvider::Mesh) => todo!(),
                        (ShaderResourceType::ImageCube, ShaderResourceProvider::World) => {
                                VkShaderResourceType::CombinedImageSampler
                        },
                        (ShaderResourceType::ImageCube, ShaderResourceProvider::Material) => {
                                VkShaderResourceType::CombinedImageSampler
                        },
                        (ShaderResourceType::ImageCube, ShaderResourceProvider::Mesh) => todo!(),
                        (ShaderResourceType::Struct(_), ShaderResourceProvider::RenderPass) => {
                                VkShaderResourceType::UniformBuffer
                        },
                        (ShaderResourceType::Image2D, ShaderResourceProvider::RenderPass) => {
                                VkShaderResourceType::CombinedImageSampler
                        },
                        (ShaderResourceType::ImageCube, ShaderResourceProvider::RenderPass) => {
                                VkShaderResourceType::CombinedImageSampler
                        },
                };

                let vk_shader_resource = VkShaderResource {
                        resource_type: vk_shader_resource_type,
                };

                self.shader_resources.insert(shader_resource_id, vk_shader_resource);

                Ok(())
        }

        fn on_shader_updated(&mut self, asset_manager: &AssetManager, shader_id: ShaderId) -> AnyResult<()> {
                let shader = match asset_manager.get_shader(shader_id) {
                        Some(shader) => shader,
                        None => return Ok(()),
                };

                let shader_resource_bindings =
                        Self::map_shader_resources(asset_manager.shader_resources(), &self.shader_resources, shader);

                let vert_shader_source = Self::complete_shader_stage_source(
                        asset_manager.shader_resources(),
                        &shader_resource_bindings,
                        &shader.vert_shader,
                );
                let frag_shader_source = shader.frag_shader.as_ref().map(|fs| {
                        Self::complete_shader_stage_source(
                                asset_manager.shader_resources(),
                                &shader_resource_bindings,
                                fs,
                        )
                });

                let vert_shader_path =
                        Self::write_generated_source_to_file(&shader.vert_shader.path, &vert_shader_source);
                let frag_shader_path = frag_shader_source.as_ref().map(|fss| {
                        Self::write_generated_source_to_file(&shader.frag_shader.as_ref().unwrap().path, fss)
                });

                let vert_shader_module = ShaderModule::from_glsl_file(vert_shader_path)?;
                let frag_shader_module = frag_shader_path
                        .map(|fsp| ShaderModule::from_glsl_file(fsp))
                        .transpose()?;

                let dst_set_layouts =
                        Self::create_descriptor_set_layouts_from_bindings(&self.device, &shader_resource_bindings)?;

                let world_dst_set_layout = dst_set_layouts[VkDescriptorSetIndex::World];
                let material_dst_set_layout = dst_set_layouts[VkDescriptorSetIndex::Material];
                let mesh_dst_set_layout = dst_set_layouts[VkDescriptorSetIndex::Mesh];

                let world_dst_set = Self::init_world_dst_set(
                        self.concurrent_frames,
                        &self.device,
                        &mut self.dst_set_allocator,
                        &self.shader_resources,
                        &self.shader_resource_buffers,
                        &shader_resource_bindings,
                        world_dst_set_layout,
                )?;

                let mesh_dst_set = Self::init_mesh_dst_set(
                        self.concurrent_frames,
                        &self.device,
                        &mut self.dst_set_allocator,
                        &self.shader_resources,
                        &self.shader_resource_dynamic_buffers,
                        &shader_resource_bindings,
                        mesh_dst_set_layout,
                )?;

                let vert_module = VkShaderModule::from_code(&self.device, &vert_shader_module.bin)?;
                let frag_module = frag_shader_module
                        .as_ref()
                        .map(|fsm| VkShaderModule::from_code(&self.device, &fsm.bin))
                        .transpose()?;

                let mut vertex_input_bindings = Vec::new();
                let mut vertex_input_attributes = Vec::new();

                for (i, vertex_input) in shader.vertex_inputs.iter().enumerate() {
                        let mut binding = vk::VertexInputBindingDescription::builder().binding(i as u32);
                        let mut attribute = vk::VertexInputAttributeDescription::builder()
                                .binding(i as u32)
                                .location(i as u32)
                                .offset(0);

                        match vertex_input.as_str() {
                                "positions" => {
                                        binding = binding.stride(std::mem::size_of::<Vec3>() as u32);
                                        binding = binding.input_rate(vk::VertexInputRate::VERTEX);
                                        attribute = attribute.format(vk::Format::R32G32B32_SFLOAT);
                                },
                                "normals" => {
                                        binding = binding.stride(std::mem::size_of::<Vec3>() as u32);
                                        binding = binding.input_rate(vk::VertexInputRate::VERTEX);
                                        attribute = attribute.format(vk::Format::R32G32B32_SFLOAT);
                                },
                                "texture-coordinates" => {
                                        binding = binding.stride(std::mem::size_of::<Vec2>() as u32);
                                        binding = binding.input_rate(vk::VertexInputRate::VERTEX);
                                        attribute = attribute.format(vk::Format::R32G32_SFLOAT);
                                },
                                "tangents" => {
                                        // NOTE: tangents are Vec4 (w is sign).
                                        binding = binding.stride(std::mem::size_of::<Vec4>() as u32);
                                        binding = binding.input_rate(vk::VertexInputRate::VERTEX);
                                        attribute = attribute.format(vk::Format::R32G32B32A32_SFLOAT);
                                },
                                _ => panic!("Invalid shader vertex input: {}", vertex_input),
                        }

                        vertex_input_bindings.push(binding.build());
                        vertex_input_attributes.push(attribute.build());
                }

                let graphics_pipeline_layout = Self::create_graphics_pipeline_layout(
                        &self.device,
                        dst_set_layouts.as_slice(),
                        shader.push_constants_size,
                )?;

                let samples = match shader.render_stage {
                        ShaderRenderStage::Drawing => self.swapchain_samples,
                        _ => vk::SampleCountFlags::TYPE_1,
                };

                let render_pass = self.render_passes[shader.render_stage];

                let graphics_pipeline = Self::create_graphics_pipeline_for_vk_shader(
                        &self.device,
                        samples,
                        render_pass,
                        *graphics_pipeline_layout,
                        !shader.disable_depth_test,
                        shader.cull_mode.into(),
                        *vert_module,
                        frag_module.as_ref().map(|fm| **fm),
                        &vertex_input_bindings,
                        &vertex_input_attributes,
                )?;

                let vk_shader = VkShader {
                        vert_module,
                        frag_module,
                        vertex_input_bindings,
                        vertex_input_attributes,
                        shader_resource_bindings,

                        world_dst_set,
                        mesh_dst_set,

                        dst_set_layouts,

                        graphics_pipeline_layout,
                        graphics_pipeline,
                };

                self.shaders.insert(shader_id, vk_shader);

                Ok(())
        }

        fn map_shader_resources(
                shader_resources: &ShaderResourceRegistry,
                vk_shader_resources: &HashMap<ShaderResourceId, VkShaderResource>,
                shader: &Shader,
        ) -> HashMap<ShaderResourceId, VkShaderResourceBindingDescription> {
                let mut next_bindings = EnumMap::from_fn(|_| 0);
                let mut bindings = HashMap::new();

                Self::map_shader_stage_resources(
                        shader_resources,
                        vk_shader_resources,
                        &shader.vert_shader,
                        &mut next_bindings,
                        &mut bindings,
                );
                if let Some(frag_shader) = shader.frag_shader.as_ref() {
                        Self::map_shader_stage_resources(
                                shader_resources,
                                vk_shader_resources,
                                frag_shader,
                                &mut next_bindings,
                                &mut bindings,
                        )
                };

                return bindings;
        }

        fn map_shader_stage_resources(
                shader_resources: &ShaderResourceRegistry,
                vk_shader_resources: &HashMap<ShaderResourceId, VkShaderResource>,
                shader_stage: &PreprocessedShaderStage,
                next_bindings: &mut EnumMap<VkDescriptorSetIndex, u32>,
                bindings: &mut HashMap<ShaderResourceId, VkShaderResourceBindingDescription>,
        ) {
                for requirement in &shader_stage.resources {
                        if bindings.contains_key(&requirement.resource_id) {
                                continue;
                        }

                        let resource = shader_resources
                                .get(&requirement.resource_id)
                                .expect(&requirement.resource_id);
                        let vk_resource = vk_shader_resources
                                .get(&requirement.resource_id)
                                .expect(&requirement.resource_id);

                        let set = match resource.provider {
                                ShaderResourceProvider::World => VkDescriptorSetIndex::World,
                                ShaderResourceProvider::RenderPass => VkDescriptorSetIndex::RenderPass,
                                ShaderResourceProvider::Material => VkDescriptorSetIndex::Material,
                                ShaderResourceProvider::Mesh => VkDescriptorSetIndex::Mesh,
                        };

                        let binding = next_bindings[set];
                        next_bindings[set] = binding + 1;

                        let descriptor_type = match vk_resource.resource_type {
                                VkShaderResourceType::UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
                                VkShaderResourceType::UniformBufferDynamic => {
                                        vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC
                                },
                                VkShaderResourceType::CombinedImageSampler => {
                                        vk::DescriptorType::COMBINED_IMAGE_SAMPLER
                                },
                        };

                        let binding_description = VkShaderResourceBindingDescription {
                                set,
                                binding,
                                descriptor_type,
                        };
                        bindings.insert(requirement.resource_id.clone(), binding_description);
                }
        }

        fn complete_shader_stage_source(
                shader_resources: &ShaderResourceRegistry,
                resource_bindings: &HashMap<ShaderResourceId, VkShaderResourceBindingDescription>,
                shader_stage: &PreprocessedShaderStage,
        ) -> String {
                let mut source_builder = ShaderStageSourceBuilder::new(&shader_stage.parts);

                for requirement in &shader_stage.resources {
                        let binding = resource_bindings.get(&requirement.resource_id).unwrap();
                        let resource = shader_resources.get(&requirement.resource_id).unwrap();
                        let type_text = resource.resource_type.glsl_complete_type();

                        let suffix = match resource.resource_type {
                                ShaderResourceType::Struct(_) => "\n",
                                _ => "",
                        };

                        let separator = format!(
                                "layout (set = {}, binding = {}) uniform {} {};\n{}",
                                binding.set.value(),
                                binding.binding,
                                type_text,
                                requirement.variable_name,
                                suffix
                        );

                        source_builder.place(requirement.separator_index, separator)
                }

                source_builder.build()
        }

        fn write_generated_source_to_file(original_path: &Path, generated_source: &str) -> PathBuf {
                let parent_dir = original_path.parent().unwrap();
                let out_dir = parent_dir.join("out");

                std::fs::create_dir_all(&out_dir).expect("failed to create out directory");

                let gen_path = out_dir.join(original_path.file_name().expect("failed to get file_name"));

                // let mut extension = OsString::from_str("gen.").unwrap();
                // extension.push(original_path.extension().unwrap());
                // let mut path = original_path.to_owned();
                // path.set_extension(extension);

                // let path = PathBuf::from_str(&(path.to_str().unwrap().to_owned() + old_extension)).unwrap();

                std::fs::write(&gen_path, generated_source).expect("failed to write shader source file");

                gen_path
        }

        // TODO: return HashMap<ShaderResourceProvider, vk::DescriptorSetLayout> (?
        fn create_descriptor_set_layouts_from_bindings(
                device: &VkDevice,
                resource_bindings: &HashMap<ShaderResourceId, VkShaderResourceBindingDescription>,
        ) -> VkResult<EnumMap<VkDescriptorSetIndex, vk::DescriptorSetLayout>> {
                let mut set_bindings = HashMap::<VkDescriptorSetIndex, Vec<vk::DescriptorSetLayoutBinding>>::new();

                for resource_binding in resource_bindings.values() {
                        let binding = vk::DescriptorSetLayoutBinding {
                                binding: resource_binding.binding,
                                descriptor_type: resource_binding.descriptor_type,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        };

                        set_bindings
                                .get_mut_or_insert_with(&resource_binding.set, || vec![])
                                .push(binding);
                }

                let set_layouts = EnumMap::from_fn(|i: VkDescriptorSetIndex| {
                        let bindings = set_bindings.remove(&i).unwrap_or_else(|| vec![]);

                        unsafe {
                                device.create_descriptor_set_layout(
                                        &vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings),
                                        None,
                                )
                        }
                });

                if let Some(err) = set_layouts.iter().find_map(|(_, r)| r.err()) {
                        return Err(err);
                }

                Ok(set_layouts.map(|_, r| r.unwrap()))
        }

        fn init_world_dst_set(
                concurrent_frames: usize,
                device: &VkDevice,
                dst_set_allocator: &mut VkDescriptorSetAllocator,
                vk_shader_resources: &HashMap<ShaderResourceId, VkShaderResource>,
                shader_resource_buffers: &HashMap<ShaderResourceId, Vec<VkBuffer>>,
                resource_bindings: &HashMap<ShaderResourceId, VkShaderResourceBindingDescription>,
                world_dst_set_layout: vk::DescriptorSetLayout,
        ) -> VkResult<Vec<vk::DescriptorSet>> {
                let dst_sets: Vec<vk::DescriptorSet> = (0..concurrent_frames)
                        .map(|_| unsafe {
                                dst_set_allocator
                                        .allocate_descriptor_sets(&[world_dst_set_layout])
                                        .map(|x| x[0])
                        })
                        .collect::<VkResult<Vec<vk::DescriptorSet>>>()?;

                for (resource_id, resource_binding) in resource_bindings {
                        if resource_binding.set != VkDescriptorSetIndex::World {
                                continue;
                        }

                        let vk_resource = vk_shader_resources.get(resource_id).unwrap();

                        match vk_resource.resource_type {
                                VkShaderResourceType::UniformBuffer => {
                                        let buffers = shader_resource_buffers.get(resource_id).expect(&format!(
                                                "no backing buffers for shader resource {}",
                                                &resource_id
                                        ));

                                        assert_eq!(dst_sets.len(), buffers.len());

                                        for (dst_set, buffer) in Iterator::zip(dst_sets.iter(), buffers.iter()) {
                                                let buffer_info = vk::DescriptorBufferInfo {
                                                        buffer: **buffer,
                                                        offset: 0,
                                                        range: vk::WHOLE_SIZE,
                                                };

                                                let dst_write = vk::WriteDescriptorSet::builder()
                                                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                                                        .dst_set(*dst_set)
                                                        .dst_binding(resource_binding.binding)
                                                        .dst_array_element(0)
                                                        .buffer_info(buffer_info.ref_into_slice());

                                                unsafe { device.update_descriptor_sets(&[dst_write.build()], &[]) };
                                        }
                                },
                                VkShaderResourceType::UniformBufferDynamic => todo!(),
                                VkShaderResourceType::CombinedImageSampler => (),
                        }
                }

                Ok(dst_sets)
        }

        fn init_mesh_dst_set(
                concurrent_frames: usize,
                device: &VkDevice,
                dst_set_allocator: &mut VkDescriptorSetAllocator,
                vk_shader_resources: &HashMap<ShaderResourceId, VkShaderResource>,
                shader_resource_dynamic_buffers: &HashMap<ShaderResourceId, Vec<VkDynamicUniformBuffer>>,
                resource_bindings: &HashMap<ShaderResourceId, VkShaderResourceBindingDescription>,
                mesh_dst_set_layout: vk::DescriptorSetLayout,
        ) -> VkResult<Vec<vk::DescriptorSet>> {
                let dst_sets: Vec<vk::DescriptorSet> = (0..concurrent_frames)
                        .map(|_| unsafe {
                                dst_set_allocator
                                        .allocate_descriptor_sets(&[mesh_dst_set_layout])
                                        .map(|x| x[0])
                        })
                        .collect::<VkResult<Vec<vk::DescriptorSet>>>()?;

                for (resource_id, resource_binding) in resource_bindings {
                        if resource_binding.set != VkDescriptorSetIndex::Mesh {
                                continue;
                        }

                        let vk_resource = vk_shader_resources.get(resource_id).unwrap();

                        match vk_resource.resource_type {
                                VkShaderResourceType::UniformBuffer => todo!(),
                                VkShaderResourceType::UniformBufferDynamic => {
                                        let buffers = shader_resource_dynamic_buffers.get(resource_id).expect(
                                                &format!("no backing buffers for shader resource {}", resource_id),
                                        );

                                        assert_eq!(dst_sets.len(), buffers.len());

                                        for (dst_set, buffer) in Iterator::zip(dst_sets.iter(), buffers.iter()) {
                                                let buffer_info = vk::DescriptorBufferInfo {
                                                        buffer: **buffer,
                                                        offset: 0,
                                                        range: buffer.element_padded_size() as vk::DeviceSize,
                                                };

                                                let dst_write = vk::WriteDescriptorSet::builder()
                                                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                                                        .dst_set(*dst_set)
                                                        .dst_binding(resource_binding.binding)
                                                        .dst_array_element(0)
                                                        .buffer_info(buffer_info.ref_into_slice());

                                                unsafe { device.update_descriptor_sets(&[dst_write.build()], &[]) };
                                        }
                                },
                                VkShaderResourceType::CombinedImageSampler => (),
                        }
                }

                Ok(dst_sets)
        }

        fn on_cubemap_updated(&mut self, asset_manager: &AssetManager, cubemap_id: CubemapId) -> AnyResult<()> {
                let cubemap = match asset_manager.get_cubemap(cubemap_id) {
                        Some(cubemap) => cubemap,
                        None => return Ok(()),
                };

                let size = match cubemap {
                        Cubemap::Faces(faces) => faces.width,
                        Cubemap::Equirectangular(image) => {
                                // We divide width by 4, as there are four horizontal faces.
                                let size = image.width / 4;
                                size
                        },
                };

                let format = match cubemap {
                        Cubemap::Faces(faces) => {
                                vk_format_from_image_format_and_color_space(faces.format, faces.color_space)
                        },
                        Cubemap::Equirectangular(_) => vk::Format::R16G16B16A16_SFLOAT,
                };

                let additional_usage_flags = match cubemap {
                        Cubemap::Faces(_) => vk::ImageUsageFlags::empty(),
                        Cubemap::Equirectangular(_) => vk::ImageUsageFlags::COLOR_ATTACHMENT,
                };

                let mut deletion_queue = vec![];

                let vk_image = unsafe {
                        match cubemap {
                                Cubemap::Faces(faces) => {
                                        // self.cmd_transfer_faces_into_cubemap(faces, &vk_image, &mut deletion_queue)?;
                                        let cinfo = VkImageCreateFromImageInfo {
                                                image: faces,
                                                mip_levels: MipLevels::Log2,
                                                setup_cmd_buffer: &self.cmd_buffer,
                                                transfer_queue: self.transfer_queue,
                                        };

                                        VkImage::from_image(
                                                &self.instance,
                                                **self.pdevice,
                                                &self.device,
                                                Rc::clone(&self.allocator),
                                                &cinfo,
                                        )?
                                },
                                Cubemap::Equirectangular(equirectangular) => {
                                        let vk_cubemap_cinfo = VkImageCubemapCreateInfo {
                                                format,
                                                size,
                                                mip_levels: MipLevels::Log2,
                                                additional_usage_flags,
                                        };

                                        let vk_image = VkImage::new_cubemap(&self.allocator, &vk_cubemap_cinfo)?;

                                        self.cmd_transfer_equirectangular_into_cubemap(
                                                asset_manager,
                                                equirectangular,
                                                &vk_image,
                                                size,
                                                &mut deletion_queue,
                                        )?;

                                        self.cmd_gen_mipmaps_for(&vk_image);

                                        self.cmd_buffer.end_and_submit(
                                                &self.device,
                                                self.transfer_queue,
                                                &[],
                                                &[],
                                                &[],
                                        )?;

                                        self.cmd_buffer.wait(u64::MAX)?;

                                        vk_image
                                },
                        }
                };

                if ENABLE_VALIDATION_LAYERS {
                        unsafe {
                                vk_image.set_debug_name(
                                        &self.device,
                                        self.debug_utils.as_ref().unwrap(),
                                        &format!("[cubemap] {:?}", cubemap_id),
                                )?;
                        }
                }

                // self.cmd_gen_mipmaps_for(&vk_image);

                let vk_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *vk_image,
                        view_type: vk::ImageViewType::CUBE,
                        format: vk_image.format,
                        components: Default::default(),
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: vk_image.mip_levels,
                                base_array_layer: 0,
                                layer_count: 6,
                        },
                        ..Default::default()
                };

                let vk_image_view = unsafe { VkImageView::new(Rc::clone(&self.device), &vk_image_view_cinfo)? };

                let vk_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: vk::Filter::LINEAR,
                        min_filter: vk::Filter::LINEAR,
                        mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                        address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: ENABLE_ANISOTROPY as u32,
                        max_anisotropy: 1.0,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::NEVER,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::INT_OPAQUE_WHITE,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                let vk_sampler = unsafe { VkSampler::new(Rc::clone(&self.device), &vk_sampler_cinfo)? };

                let irradiance_image;
                let prefiltered_image;

                unsafe {
                        self.cmd_buffer.begin(&self.device)?;

                        irradiance_image = self.gen_irrandiace_map_for(
                                asset_manager,
                                &vk_image,
                                &vk_image_view,
                                &mut deletion_queue,
                        )?;

                        prefiltered_image = self.gen_prefiltered_map_for(
                                asset_manager,
                                &vk_image,
                                &vk_image_view,
                                &mut deletion_queue,
                        )?;

                        if ENABLE_VALIDATION_LAYERS {
                                irradiance_image.set_debug_name(
                                        &self.device,
                                        self.debug_utils.as_ref().unwrap(),
                                        &format!("[cubemap] {:?} (irradiance)", cubemap_id),
                                )?;

                                prefiltered_image.set_debug_name(
                                        &self.device,
                                        self.debug_utils.as_ref().unwrap(),
                                        &format!("[cubemap] {:?} (prefiltered)", cubemap_id),
                                )?;
                        }

                        self.cmd_buffer
                                .end_and_submit(&self.device, self.transfer_queue, &[], &[], &[])?;

                        self.cmd_buffer.wait(u64::MAX)?;

                        for o in deletion_queue.into_iter().rev() {
                                o.destroy();
                        }
                }

                let irradiance_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *irradiance_image,
                        view_type: vk::ImageViewType::CUBE,
                        format: irradiance_image.format,
                        components: Default::default(),
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: irradiance_image.mip_levels,
                                base_array_layer: 0,
                                layer_count: 6,
                        },
                        ..Default::default()
                };

                let irradiance_image_view =
                        unsafe { VkImageView::new(Rc::clone(&self.device), &irradiance_image_view_cinfo)? };

                let irradiance_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: vk::Filter::LINEAR,
                        min_filter: vk::Filter::LINEAR,
                        mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                        address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: ENABLE_ANISOTROPY as u32,
                        max_anisotropy: 1.0,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::NEVER,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::INT_OPAQUE_WHITE,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                let irradiance_sampler = unsafe { VkSampler::new(Rc::clone(&self.device), &irradiance_sampler_cinfo)? };

                let prefiltered_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *prefiltered_image,
                        view_type: vk::ImageViewType::CUBE,
                        format: prefiltered_image.format,
                        components: Default::default(),
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: prefiltered_image.mip_levels,
                                base_array_layer: 0,
                                layer_count: 6,
                        },
                        ..Default::default()
                };

                let prefiltered_image_view =
                        unsafe { VkImageView::new(Rc::clone(&self.device), &prefiltered_image_view_cinfo)? };

                let prefiltered_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: vk::Filter::LINEAR,
                        min_filter: vk::Filter::LINEAR,
                        mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                        address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: ENABLE_ANISOTROPY as u32,
                        max_anisotropy: 1.0,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::NEVER,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::INT_OPAQUE_WHITE,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                let prefiltered_sampler =
                        unsafe { VkSampler::new(Rc::clone(&self.device), &prefiltered_sampler_cinfo)? };

                let vk_cubemap = VkCubemap {
                        environment_image: vk_image,
                        environment_image_view: vk_image_view,
                        environment_sampler: vk_sampler,
                        irradiance_image,
                        irradiance_image_view,
                        irradiance_sampler,
                        prefiltered_image,
                        prefiltered_image_view,
                        prefiltered_sampler,
                };

                self.cubemaps.insert(cubemap_id, vk_cubemap);

                Ok(())
        }

        // unsafe fn cmd_transfer_faces_into_cubemap(
        //         &self,
        //         faces: &[Image; 6],
        //         cubemap: &VkImage,
        //         cmd_buffer_deletion_queue: &mut Vec<VkObject>,
        // ) -> VkResult<()> {
        //         unsafe { self.cmd_buffer.begin(&self.device)? };

        //         VkImage::cmd_transition_img_layout(&TransitionImageLayoutInfo {
        //                 device: &self.device,
        //                 cmd_buffer: *self.cmd_buffer,

        //                 old_layout: vk::ImageLayout::UNDEFINED,
        //                 new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

        //                 image: **cubemap,
        //                 base_mip_level: 0,
        //                 mip_levels: cubemap.mip_levels,
        //                 base_array_layer: 0,
        //                 layer_count: 6,
        //                 aspect_mask: vk::ImageAspectFlags::COLOR,

        //                 src_access_mask: vk::AccessFlags::empty(),
        //                 dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

        //                 src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
        //                 dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
        //         });

        //         let buffer_size =
        //                 6 * (cubemap.width * cubemap.height * cubemap.format.bytes_per_pixel()) as vk::DeviceSize;
        //         let staging_buffer = VkBuffer::new_transfer_src(&self.device, Rc::clone(&self.allocator), buffer_size)?;

        //         let mut offset = 0;
        //         for face in faces {
        //                 let face_data = face.data.as_slice();
        //                 staging_buffer.write_bytes_offsetted(face_data, offset)?;
        //                 offset += face_data.len();
        //         }
        //         staging_buffer.unmap_memory();

        //         VkImage::cmd_copy_buffer_to_image(
        //                 &self.device,
        //                 *self.cmd_buffer,
        //                 cubemap.width,
        //                 cubemap.height,
        //                 6,
        //                 *staging_buffer,
        //                 **cubemap,
        //                 vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        //         );

        //         cmd_buffer_deletion_queue.push(VkObject::Buffer(staging_buffer));

        //         Ok(())
        // }

        /// `cubemap_image` must be in vk::Format::R16G16B16A16_SFLOAT format
        unsafe fn cmd_transfer_equirectangular_into_cubemap(
                &self,
                asset_manager: &AssetManager,
                equirectangular_image: &Image,
                cubemap_image: &VkImage,
                size: u32,
                deletion_queue: &mut Vec<VkObject>,
        ) -> VkResult<()> {
                assert_eq!(cubemap_image.format, vk::Format::R16G16B16A16_SFLOAT);

                let equirectangular_cinfo = VkImageCreateFromImageInfo {
                        image: equirectangular_image,
                        mip_levels: MipLevels::N(1),
                        setup_cmd_buffer: &self.cmd_buffer,
                        transfer_queue: self.transfer_queue,
                };

                let equirectangular_vk_image = VkImage::from_image(
                        &self.instance,
                        **self.pdevice,
                        &self.device,
                        Rc::clone(&self.allocator),
                        &equirectangular_cinfo,
                )?;

                let equirectangular_vk_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *equirectangular_vk_image,
                        view_type: vk::ImageViewType::TYPE_2D,
                        format: equirectangular_vk_image.format,
                        components: Default::default(),
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: equirectangular_vk_image.mip_levels,
                                base_array_layer: 0,
                                layer_count: 1,
                        },
                        ..Default::default()
                };

                let equirectangular_vk_image_view =
                        unsafe { VkImageView::new(Rc::clone(&self.device), &equirectangular_vk_image_view_cinfo)? };

                let equirectangular_vk_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: vk::Filter::LINEAR,
                        min_filter: vk::Filter::LINEAR,
                        mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                        address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: ENABLE_ANISOTROPY as u32,
                        max_anisotropy: 1.0,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::NEVER,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::INT_OPAQUE_WHITE,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                let equirectangular_vk_sampler =
                        unsafe { VkSampler::new(Rc::clone(&self.device), &equirectangular_vk_sampler_cinfo)? };

                let equi_to_cube_shader_id = asset_manager.shader_names()["equi-to-cube-shader"];
                let equi_to_cube_vk_shader = &self.shaders[equi_to_cube_shader_id];

                let equirectangular_binding = equi_to_cube_vk_shader
                        .shader_resource_bindings
                        .get(&SHADER_RESOURCE_EQUIRECTANGULAR_MAP)
                        .unwrap();

                let image_info = vk::DescriptorImageInfo {
                        sampler: *equirectangular_vk_sampler,
                        image_view: *equirectangular_vk_image_view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                };

                deletion_queue.push(VkObject::Image(equirectangular_vk_image));
                deletion_queue.push(VkObject::ImageView(equirectangular_vk_image_view));
                deletion_queue.push(VkObject::Sampler(equirectangular_vk_sampler));

                let write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .dst_set(equi_to_cube_vk_shader.world_dst_set[0])
                        .dst_binding(equirectangular_binding.binding)
                        .dst_array_element(0)
                        .image_info(image_info.ref_into_slice())
                        .build();

                self.device.update_descriptor_sets(&[write], &[]);

                let rotations = [
                        Mat4::from_rotation_y(std::f32::consts::TAU / 4.0),
                        Mat4::from_rotation_y(-std::f32::consts::TAU / 4.0),
                        Mat4::from_rotation_x(-std::f32::consts::TAU / 4.0),
                        Mat4::from_rotation_x(std::f32::consts::TAU / 4.0),
                        Mat4::IDENTITY,
                        Mat4::from_rotation_y(std::f32::consts::TAU / 2.0),
                ];

                unsafe { self.cmd_buffer.begin(&self.device)? };
                let cmd_buffer = *self.cmd_buffer;

                // transition all mips of cubemap_image into TRANSFER_DST_OPTIMAL layout.
                // note: the transition for the 0th mip is unnecessary, as the transition
                // could be done implicitly by the render pass.
                VkImage::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                        device: &self.device,
                        cmd_buffer: *self.cmd_buffer,

                        old_layout: vk::ImageLayout::UNDEFINED,
                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

                        image: **cubemap_image,
                        base_mip_level: 0,
                        mip_levels: cubemap_image.mip_levels,
                        base_array_layer: 0,
                        layer_count: 6,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

                        src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
                        dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                });

                self.device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *equi_to_cube_vk_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::World.value(),
                        &[equi_to_cube_vk_shader.world_dst_set[0]],
                        &[],
                );

                let scissor = vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                                width: size,
                                height: size,
                        },
                };

                let viewport = vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: size as f32,
                        height: size as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                self.device.cmd_set_scissor(cmd_buffer, 0, scissor.ref_into_slice());
                self.device.cmd_set_viewport(cmd_buffer, 0, viewport.ref_into_slice());

                self.device.cmd_bind_pipeline(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *equi_to_cube_vk_shader.graphics_pipeline,
                );

                for i in 0..6usize {
                        let face_image_view_cinfo = vk::ImageViewCreateInfo::builder()
                                .image(**cubemap_image)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(vk::Format::R16G16B16A16_SFLOAT)
                                .components(Default::default())
                                .subresource_range(vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: i as u32,
                                        layer_count: 1,
                                });

                        let face_image_view = VkImageView::new(Rc::clone(&self.device), &face_image_view_cinfo)?;

                        let attachments = [*face_image_view];
                        let render_pass = self.render_passes[ShaderRenderStage::SkyboxMapping];

                        let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                .render_pass(render_pass)
                                .width(size)
                                .height(size)
                                .layers(1)
                                .attachments(&attachments);

                        let framebuffer = VkFramebuffer::new(&self.device, &framebuffer_cinfo)?;

                        let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                .render_pass(render_pass)
                                .framebuffer(*framebuffer)
                                .render_area(vk::Rect2D {
                                        offset: vk::Offset2D { x: 0, y: 0 },
                                        extent: vk::Extent2D {
                                                width: size,
                                                height: size,
                                        },
                                });

                        self.device
                                .cmd_begin_render_pass(cmd_buffer, &render_pass_binfo, vk::SubpassContents::INLINE);

                        self.device.cmd_push_constants(
                                cmd_buffer,
                                *equi_to_cube_vk_shader.graphics_pipeline_layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                rotations[i].as_bytes(),
                        );

                        self.device.cmd_draw(cmd_buffer, 3, 1, 0, 0);

                        self.device.cmd_end_render_pass(cmd_buffer);

                        deletion_queue.push(VkObject::ImageView(face_image_view));
                        deletion_queue.push(VkObject::Framebuffer(framebuffer));
                }

                Ok(())
        }

        fn cmd_gen_mipmaps_for(&self, image: &VkImage) {
                VkImage::cmd_gen_mipmaps(&GenerateMipmapsInfo {
                        instance: &self.instance,
                        pdevice: **self.pdevice,
                        device: &self.device,
                        cmd_buffer: *self.cmd_buffer,
                        image: image.handle,
                        image_format: image.format,
                        width: image.width,
                        height: image.height,
                        mip_levels: image.mip_levels,
                        base_array_layer: 0,
                        layer_count: image.array_layers,
                });
        }

        unsafe fn gen_irrandiace_map_for(
                &self,
                asset_manager: &AssetManager,
                environment_image: &VkImage,
                environment_image_view: &VkImageView,
                deletion_queue: &mut Vec<VkObject>,
        ) -> VkResult<VkImage> {
                let format = vk::Format::R16G16B16A16_SFLOAT;
                assert_eq!(environment_image.width, environment_image.height);
                let size = 32;

                // NOTE: we generate a new sampler for the environment map due to the problem described in max_lod
                let environment_sampler = {
                        let environment_sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: vk::Filter::LINEAR,
                                min_filter: vk::Filter::LINEAR,
                                mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                                address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                                address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                                address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                                mip_lod_bias: 0.0,
                                anisotropy_enable: ENABLE_ANISOTROPY as u32,
                                max_anisotropy: 1.0,
                                compare_enable: vk::FALSE,
                                compare_op: vk::CompareOp::NEVER,
                                min_lod: 0.0,
                                // NOTE: for some reason using mipmaps when calculating the irradiance
                                // map produces ugly artifacts. Because of this we only use mip 0.
                                // Maybe we shouldn't even generate mipmaps for the environment map?
                                max_lod: 0.0,
                                border_color: vk::BorderColor::INT_OPAQUE_WHITE,
                                unnormalized_coordinates: vk::FALSE,
                                ..Default::default()
                        };

                        unsafe { VkSampler::new(Rc::clone(&self.device), &environment_sampler_cinfo)? }
                };

                let irradiance_image_cinfo = VkImageCubemapCreateInfo {
                        format,
                        size,
                        mip_levels: MipLevels::Log2,
                        additional_usage_flags: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                };

                let irradiance_image = VkImage::new_cubemap(&self.allocator, &irradiance_image_cinfo)?;

                let irradiance_shader_id = asset_manager.shader_names()["irradiance-shader"];
                let irradiance_shader = &self.shaders[irradiance_shader_id];

                let environment_map_binding = irradiance_shader
                        .shader_resource_bindings
                        .get(&SHADER_RESOURCE_ENVIRONMENT_MAP)
                        .unwrap();

                let image_info = vk::DescriptorImageInfo {
                        sampler: *environment_sampler,
                        image_view: **environment_image_view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                };

                deletion_queue.push(VkObject::Sampler(environment_sampler));

                let write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .dst_set(irradiance_shader.world_dst_set[0])
                        .dst_binding(environment_map_binding.binding)
                        .dst_array_element(0)
                        .image_info(image_info.ref_into_slice())
                        .build();

                self.device.update_descriptor_sets(&[write], &[]);

                let cmd_buffer = *self.cmd_buffer;

                VkImage::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                        device: &self.device,
                        cmd_buffer: *self.cmd_buffer,

                        old_layout: vk::ImageLayout::UNDEFINED,
                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

                        image: *irradiance_image,
                        base_mip_level: 0,
                        mip_levels: irradiance_image.mip_levels,
                        base_array_layer: 0,
                        layer_count: 6,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

                        src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
                        dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                });

                self.device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *irradiance_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::World.value(),
                        &[irradiance_shader.world_dst_set[0]],
                        &[],
                );

                self.cmd_render_cubemap(cmd_buffer, irradiance_shader, &irradiance_image, 0, deletion_queue)?;

                self.cmd_gen_mipmaps_for(&irradiance_image);

                Ok(irradiance_image)
        }

        unsafe fn gen_prefiltered_map_for(
                &mut self,
                asset_manager: &AssetManager,
                environment_image: &VkImage,
                environment_image_view: &VkImageView,
                deletion_queue: &mut Vec<VkObject>,
        ) -> VkResult<VkImage> {
                let format = vk::Format::R16G16B16A16_SFLOAT;
                assert_eq!(environment_image.width, environment_image.height);
                let size = PREFILTER_MAP_SIZE;

                let environment_sampler = {
                        let environment_sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: vk::Filter::LINEAR,
                                min_filter: vk::Filter::LINEAR,
                                mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                                address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                                address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                                address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                                mip_lod_bias: 0.0,
                                anisotropy_enable: ENABLE_ANISOTROPY as u32,
                                max_anisotropy: 1.0,
                                compare_enable: vk::FALSE,
                                compare_op: vk::CompareOp::NEVER,
                                min_lod: 0.0,
                                // we manually specify LOD in shader, therefore this doesn't
                                // causes the problems mentioned for irradiance maps
                                max_lod: vk::LOD_CLAMP_NONE,
                                border_color: vk::BorderColor::INT_OPAQUE_WHITE,
                                unnormalized_coordinates: vk::FALSE,
                                ..Default::default()
                        };

                        unsafe { VkSampler::new(Rc::clone(&self.device), &environment_sampler_cinfo)? }
                };

                let prefiltered_image_cinfo = VkImageCubemapCreateInfo {
                        format,
                        size,
                        mip_levels: MipLevels::Log2, // As many mip levels as possible
                        additional_usage_flags: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                };

                let prefiltered_image = VkImage::new_cubemap(&self.allocator, &prefiltered_image_cinfo)?;

                let prefilter_shader_id = asset_manager.shader_names()["prefilter-shader"];
                let prefilter_shader = &self.shaders[prefilter_shader_id];

                let environment_map_binding = prefilter_shader
                        .shader_resource_bindings
                        .get(&SHADER_RESOURCE_ENVIRONMENT_MAP)
                        .unwrap();

                let prefilter_params_binding = prefilter_shader
                        .shader_resource_bindings
                        .get(&SHADER_RESOURCE_PREFILTER_PARAMS)
                        .unwrap();

                let image_info = vk::DescriptorImageInfo {
                        sampler: *environment_sampler,
                        image_view: **environment_image_view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                };

                deletion_queue.push(VkObject::Sampler(environment_sampler));

                let write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .dst_set(prefilter_shader.world_dst_set[0])
                        .dst_binding(environment_map_binding.binding)
                        .dst_array_element(0)
                        .image_info(image_info.ref_into_slice())
                        .build();

                self.device.update_descriptor_sets(&[write], &[]);

                let cmd_buffer = *self.cmd_buffer;

                VkImage::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                        device: &self.device,
                        cmd_buffer: *self.cmd_buffer,

                        old_layout: vk::ImageLayout::UNDEFINED,
                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

                        image: *prefiltered_image,
                        base_mip_level: 0,
                        mip_levels: prefiltered_image.mip_levels,
                        base_array_layer: 0,
                        layer_count: 6,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

                        src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
                        dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                });

                self.device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *prefilter_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::World.value(),
                        &[prefilter_shader.world_dst_set[0]],
                        &[],
                );

                for mip in 0..prefiltered_image.mip_levels {
                        let roughness = (mip as f32) / ((prefiltered_image.mip_levels - 1) as f32);

                        let params_buffer_size = std::mem::size_of::<PrefilterParams>() as vk::DeviceSize;
                        let params_buffer = VkBuffer::new_uniform_buffer(
                                &self.device,
                                Rc::clone(&self.allocator),
                                params_buffer_size,
                        )?;

                        let params = PrefilterParams {
                                roughness_and_env_map_size: Vec2::new(roughness, environment_image.width as f32),
                        };
                        params_buffer.write(&params)?;

                        let render_pass_dst_set_layout =
                                prefilter_shader.dst_set_layouts[VkDescriptorSetIndex::RenderPass];
                        let [render_pass_dst_set] = self.frame_dst_set_allocators[0]
                                .allocate_descriptor_sets(&[render_pass_dst_set_layout])?;

                        let buffer_info = vk::DescriptorBufferInfo {
                                buffer: *params_buffer,
                                offset: 0,
                                range: params_buffer_size,
                        };

                        let write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(prefilter_params_binding.descriptor_type)
                                .dst_set(render_pass_dst_set)
                                .dst_binding(prefilter_params_binding.binding)
                                .dst_array_element(0)
                                .buffer_info(buffer_info.ref_into_slice())
                                .build();

                        self.device.update_descriptor_sets(&[write], &[]);

                        deletion_queue.push(VkObject::Buffer(params_buffer));

                        self.device.cmd_bind_descriptor_sets(
                                cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *prefilter_shader.graphics_pipeline_layout,
                                VkDescriptorSetIndex::RenderPass.value(),
                                &[render_pass_dst_set],
                                &[],
                        );

                        self.cmd_render_cubemap(cmd_buffer, prefilter_shader, &prefiltered_image, mip, deletion_queue)?;
                }

                VkImage::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                        device: &self.device,
                        cmd_buffer: *self.cmd_buffer,

                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,

                        image: *prefiltered_image,
                        base_mip_level: 0,
                        mip_levels: prefiltered_image.mip_levels,
                        base_array_layer: 0,
                        layer_count: 6,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        src_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                        dst_access_mask: vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE,

                        src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                        dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                });

                Ok(prefiltered_image)
        }

        unsafe fn cmd_render_cubemap(
                &self,
                cmd_buffer: vk::CommandBuffer,
                shader: &VkShader,
                target: &VkImage,
                mip_level: u32,
                deletion_queue: &mut Vec<VkObject>,
        ) -> VkResult<()> {
                // calculate size of mip
                let size = (target.width as f32 * 0.5f32.powi(mip_level as i32)) as u32;

                let scissor = vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                                width: size,
                                height: size,
                        },
                };

                let viewport = vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: size as f32,
                        height: size as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                self.device.cmd_set_scissor(cmd_buffer, 0, scissor.ref_into_slice());
                self.device.cmd_set_viewport(cmd_buffer, 0, viewport.ref_into_slice());

                self.device
                        .cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::GRAPHICS, *shader.graphics_pipeline);

                let rotations = [
                        Mat4::from_rotation_y(std::f32::consts::TAU / 4.0),  // +X (right)
                        Mat4::from_rotation_y(-std::f32::consts::TAU / 4.0), // -X (left)
                        Mat4::from_rotation_x(-std::f32::consts::TAU / 4.0), // +Y (up)
                        Mat4::from_rotation_x(std::f32::consts::TAU / 4.0),  // -Y (down)
                        Mat4::IDENTITY,                                      // +Z (forward)
                        Mat4::from_rotation_y(std::f32::consts::TAU / 2.0),  // -Z (backward)
                ];

                for i in 0..6usize {
                        let face_image_view_cinfo = vk::ImageViewCreateInfo::builder()
                                .image(**target)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(target.format)
                                .components(Default::default())
                                .subresource_range(vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: mip_level,
                                        level_count: 1,
                                        base_array_layer: i as u32,
                                        layer_count: 1,
                                });

                        let face_image_view = VkImageView::new(Rc::clone(&self.device), &face_image_view_cinfo)?;

                        let render_pass = self.render_passes[ShaderRenderStage::SkyboxMapping];
                        let attachments = [*face_image_view];

                        let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                .render_pass(render_pass)
                                .width(size)
                                .height(size)
                                .layers(1)
                                .attachments(&attachments);

                        let framebuffer = VkFramebuffer::new(&self.device, &framebuffer_cinfo)?;

                        let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                .render_pass(render_pass)
                                .framebuffer(*framebuffer)
                                .render_area(vk::Rect2D {
                                        offset: vk::Offset2D { x: 0, y: 0 },
                                        extent: vk::Extent2D {
                                                width: size,
                                                height: size,
                                        },
                                });

                        self.device
                                .cmd_begin_render_pass(cmd_buffer, &render_pass_binfo, vk::SubpassContents::INLINE);

                        self.device.cmd_push_constants(
                                cmd_buffer,
                                *shader.graphics_pipeline_layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                rotations[i].as_bytes(),
                        );

                        self.device.cmd_draw(cmd_buffer, 3, 1, 0, 0);

                        self.device.cmd_end_render_pass(cmd_buffer);

                        deletion_queue.push(VkObject::ImageView(face_image_view));
                        deletion_queue.push(VkObject::Framebuffer(framebuffer));
                }

                Ok(())
        }

        fn create_vk_mesh(&mut self, asset_manager: &AssetManager, mesh_id: MeshId) -> AnyResult<()> {
                let mesh = match asset_manager.get_mesh(mesh_id) {
                        Some(mesh) => mesh,
                        None => return Ok(()),
                };

                let positions = self.create_vk_vertex_buffer(&mesh.positions)?;
                let tex_coords = self.create_vk_vertex_buffer(&mesh.tex_coords)?;
                let normals = self.create_vk_vertex_buffer(&mesh.normals)?;
                let tangents = self.create_vk_vertex_buffer(&mesh.tangents)?;
                let indices = match &mesh.indices {
                        IndicesVec::U16(indices) => self.create_vk_index_buffer(indices)?,
                        IndicesVec::U32(indices) => self.create_vk_index_buffer(indices)?,
                };

                self.meshes.insert(
                        mesh_id,
                        VkMesh {
                                positions,
                                tex_coords,
                                normals,
                                tangents,
                                indices,
                        },
                );

                Ok(())
        }

        fn create_vk_vertex_buffer<T>(&mut self, data: &[T]) -> AnyResult<VkBuffer> {
                self.create_vk_buffer(data, BufferUsageFlags::VERTEX_BUFFER)
        }

        fn create_vk_index_buffer<T: VkIndex>(&mut self, data: &[T]) -> AnyResult<VkIndexBuffer> {
                let buffer = self.create_vk_buffer(data, BufferUsageFlags::INDEX_BUFFER)?;

                Ok(VkIndexBuffer {
                        buffer,
                        index_type: T::index_type(),
                        index_count: data.len() as u32,
                })
        }

        fn create_vk_buffer<T>(&mut self, data: &[T], buffer_usage: BufferUsageFlags) -> AnyResult<VkBuffer> {
                let vk_buffer_cinfo = VkImmutableBufferCreateInfo {
                        device: &self.device,
                        allocator: Rc::clone(&self.allocator),
                        cmd_buffer: &self.cmd_buffer,
                        transfer_queue: self.transfer_queue,
                        buffer_usage,
                        data: BufferData::FullSlice(data),
                };

                VkBuffer::new_immutable(vk_buffer_cinfo)
        }

        fn create_vk_image_from_image(
                &self,
                asset_manager: &AssetManager,
                image_id: ImageId,
        ) -> AnyResult<Option<VkModelImage>> {
                let image = match asset_manager.get_image(image_id) {
                        Some(image) => image,
                        None => return Ok(None),
                };

                let vk_image_cinfo = VkImageCreateFromImageInfo {
                        image,
                        mip_levels: MipLevels::Log2,
                        setup_cmd_buffer: &self.cmd_buffer,
                        transfer_queue: self.transfer_queue,
                };

                let vk_image = unsafe {
                        VkImage::from_image(
                                &self.instance,
                                **self.pdevice,
                                &self.device,
                                Rc::clone(&self.allocator),
                                &vk_image_cinfo,
                        )?
                };

                if ENABLE_VALIDATION_LAYERS {
                        let name = match &image.name {
                                Some(name) => format!("[image] {}", name),
                                None => format!("[image] {:?}", image_id),
                        };

                        unsafe { vk_image.set_debug_name(&self.device, self.debug_utils.as_ref().unwrap(), &name)? };
                }

                let vk_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *vk_image,
                        view_type: vk::ImageViewType::TYPE_2D,
                        format: vk_image.format,
                        components: Default::default(),
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: vk_image.mip_levels,
                                base_array_layer: 0,
                                layer_count: 1,
                        },
                        ..Default::default()
                };

                let vk_image_view = unsafe { VkImageView::new(Rc::clone(&self.device), &vk_image_view_cinfo)? };

                Ok(Some(VkModelImage {
                        image: vk_image,
                        image_view: vk_image_view,
                }))
        }

        fn create_vk_sampler_from_sampler(
                &self,
                asset_manager: &AssetManager,
                sampler_id: SamplerId,
        ) -> AnyResult<Option<VkSampler>> {
                let sampler = match asset_manager.get_sampler(sampler_id) {
                        Some(sampler) => sampler,
                        None => return Ok(None),
                };

                let vk_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: Self::vk_filter_from_mag_filter(sampler.mag_filter),
                        min_filter: Self::vk_filter_from_min_filter(sampler.min_filter),
                        mipmap_mode: Self::vk_sampler_mipmap_mode_from_min_filter(sampler.min_filter),
                        address_mode_u: Self::vk_sampler_address_mode_from_wrapping_mode(sampler.wrap_s),
                        address_mode_v: Self::vk_sampler_address_mode_from_wrapping_mode(sampler.wrap_t),
                        address_mode_w: vk::SamplerAddressMode::REPEAT,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: ENABLE_ANISOTROPY as vk::Bool32,
                        max_anisotropy: self.pdevice.max_sampler_anisotropy,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::ALWAYS,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                let vk_sampler = unsafe { VkSampler::new(Rc::clone(&self.device), &vk_sampler_cinfo)? };

                Ok(Some(vk_sampler))
        }

        fn create_vk_material_from_material(
                &mut self,
                asset_manager: &AssetManager,
                material_id: MaterialId,
        ) -> AnyResult<Option<VkMaterial>> {
                let Some(material) = asset_manager.get_material(material_id) else {
                        return Ok(None);
                };

                let Some(vk_shader) = self.shaders.get(material.shader) else {
                        return Ok(None);
                };

                let dst_sets = (0..self.concurrent_frames)
                        .map(|_| unsafe {
                                self.dst_set_allocator
                                        .allocate_descriptor_sets(&[
                                                vk_shader.dst_set_layouts[VkDescriptorSetIndex::Material]
                                        ])
                                        .map(|x| x[0])
                        })
                        .collect::<VkResult<Vec<vk::DescriptorSet>>>()?;

                let mut buffers: HashMap<ShaderResourceId, Vec<VkBuffer>> = HashMap::new();

                for (resource_id, resource_binding) in &vk_shader.shader_resource_bindings {
                        if resource_binding.set != VkDescriptorSetIndex::Material {
                                continue;
                        }

                        let resource = asset_manager.shader_resources().get(resource_id).unwrap();
                        let vk_resource = self.shader_resources.get(resource_id).unwrap();

                        let mut resource_buffers = vec![];

                        for &dst_set in &dst_sets {
                                let write = vk::WriteDescriptorSet::builder()
                                        .descriptor_type(vk_resource.resource_type.descriptor_type())
                                        .dst_set(dst_set)
                                        .dst_binding(resource_binding.binding)
                                        .dst_array_element(0);

                                match vk_resource.resource_type {
                                        VkShaderResourceType::UniformBuffer => {
                                                let ShaderResourceType::Struct(declaration) = &resource.resource_type
                                                else {
                                                        panic!()
                                                };

                                                let buffer = VkBuffer::new_uniform_buffer(
                                                        &self.device,
                                                        Rc::clone(&self.allocator),
                                                        declaration.compute_size() as vk::DeviceSize,
                                                )?;

                                                let buffer_info = vk::DescriptorBufferInfo {
                                                        buffer: *buffer,
                                                        offset: 0,
                                                        range: vk::WHOLE_SIZE,
                                                };

                                                resource_buffers.push(buffer);

                                                let write = write.buffer_info(buffer_info.ref_into_slice());
                                                unsafe { self.device.update_descriptor_sets(&[write.build()], &[]) };
                                        },
                                        VkShaderResourceType::UniformBufferDynamic => panic!("{}", resource_id),
                                        VkShaderResourceType::CombinedImageSampler => {
                                                material.get_shader_resource_data(resource_id, |data| {
                                                        let data = data
                                                                .expect(&format!("material does not have resource {}", resource_id));

                                                        match data {
                                                                ShaderResourceData::Image2D(texture_id) =>  {
                                                                        let texture = asset_manager.texture(texture_id);

                                                                        let image_info = vk::DescriptorImageInfo {
                                                                                sampler: *self.samplers[texture.sampler],
                                                                                image_view: *self.images[texture.image].image_view,
                                                                                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                                                        };

                                                                        let write = write.image_info(image_info.ref_into_slice());
                                                                        unsafe { self.device.update_descriptor_sets(&[write.build()], &[]) };
                                                                },
                                                           _ => panic!("invalid resource data type {:?}", data),
                                                        }
                                                });
                                        },
                                };
                        }

                        buffers.insert(resource_id.clone(), resource_buffers);
                }

                let vk_material = VkMaterial { dst_sets, buffers };

                Ok(Some(vk_material))
        }

        fn create_graphics_pipeline_for_vk_shader(
                device: &Rc<VkDevice>,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: vk::PipelineLayout,
                enable_depth_test: bool,
                cull_mode: vk::CullModeFlags,
                vert_module: vk::ShaderModule,
                frag_module: Option<vk::ShaderModule>,
                vertex_input_bindings: &[vk::VertexInputBindingDescription],
                vertex_input_attributes: &[vk::VertexInputAttributeDescription],
        ) -> VkResult<VkPipeline> {
                let entry_point = CString::new("main").unwrap();

                let mut shader_stages = vec![];

                shader_stages.push(vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::VERTEX)
                        .module(vert_module)
                        .name(&entry_point)
                        .build());

                if let Some(frag_module) = frag_module {
                        shader_stages.push(vk::PipelineShaderStageCreateInfo::builder()
                                .stage(vk::ShaderStageFlags::FRAGMENT)
                                .module(frag_module)
                                .name(&entry_point)
                                .build());
                }
                let mut vert_input_cinfo = vk::PipelineVertexInputStateCreateInfo::builder();

                if !vertex_input_bindings.is_empty() {
                        vert_input_cinfo = vert_input_cinfo.vertex_binding_descriptions(&vertex_input_bindings);
                }

                if !vertex_input_attributes.is_empty() {
                        vert_input_cinfo = vert_input_cinfo.vertex_attribute_descriptions(&vertex_input_attributes);
                }

                let input_assembly_cinfo = vk::PipelineInputAssemblyStateCreateInfo::builder()
                        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
                        .primitive_restart_enable(false);

                let viewport = vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                let scissor = vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D { width: 1, height: 1 },
                };

                let viewport_state_cinfo = vk::PipelineViewportStateCreateInfo::builder()
                        .viewports(viewport.ref_into_slice())
                        .scissors(scissor.ref_into_slice());

                let rasterization_state_cinfo = vk::PipelineRasterizationStateCreateInfo::builder()
                        .depth_clamp_enable(false)
                        .rasterizer_discard_enable(false)
                        .polygon_mode(vk::PolygonMode::FILL)
                        .line_width(1.0)
                        .cull_mode(cull_mode)
                        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                        .depth_bias_enable(false)
                        .depth_bias_constant_factor(0.0)
                        .depth_bias_clamp(0.0)
                        .depth_bias_slope_factor(0.0);

                let multisample_state_cinfo = vk::PipelineMultisampleStateCreateInfo::builder()
                        .rasterization_samples(swapchain_samples)
                        .sample_shading_enable(false);

                let depth_stencil_state_cinfo = vk::PipelineDepthStencilStateCreateInfo::builder()
                        .depth_test_enable(enable_depth_test)
                        .depth_write_enable(true)
                        .depth_compare_op(vk::CompareOp::LESS)
                        .depth_bounds_test_enable(false)
                        .stencil_test_enable(false);

                let color_blend_attachments = [vk::PipelineColorBlendAttachmentState::builder()
                        .color_write_mask(vk::ColorComponentFlags::RGBA)
                        .blend_enable(false)
                        .build()];

                let color_blend_state_cinfo = vk::PipelineColorBlendStateCreateInfo::builder()
                        .attachments(&color_blend_attachments)
                        .logic_op_enable(false);

                let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
                let pipeline_dyn_state_cinfo =
                        vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dyn_states);

                let graphics_pipeline_cinfo = vk::GraphicsPipelineCreateInfo::builder()
                        .stages(&shader_stages)
                        .vertex_input_state(&vert_input_cinfo)
                        .input_assembly_state(&input_assembly_cinfo)
                        .viewport_state(&viewport_state_cinfo)
                        .rasterization_state(&rasterization_state_cinfo)
                        .multisample_state(&multisample_state_cinfo)
                        .depth_stencil_state(&depth_stencil_state_cinfo)
                        .color_blend_state(&color_blend_state_cinfo)
                        .dynamic_state(&pipeline_dyn_state_cinfo)
                        .layout(pipeline_layout)
                        .render_pass(render_pass)
                        .subpass(0);

                unsafe { VkPipeline::new_graphics(device, vk::PipelineCache::null(), &graphics_pipeline_cinfo.build()) }
        }

        // fn vk_format_from_component_and_data_type(comp_type: ComponentType, data_type: DataType) -> vk::Format {
        //         match (comp_type, data_type) {
        //                 (ComponentType::I8, DataType::Scalar) => vk::Format::R8_SINT,
        //                 (ComponentType::I16, DataType::Scalar) => vk::Format::R16_SINT,
        //                 (ComponentType::U8, DataType::Scalar) => vk::Format::R8_UINT,
        //                 (ComponentType::U16, DataType::Scalar) => vk::Format::R16_UINT,
        //                 (ComponentType::U32, DataType::Scalar) => vk::Format::R32_UINT,
        //                 (ComponentType::F32, DataType::Scalar) => vk::Format::R32_SFLOAT,

        //                 (ComponentType::I8, DataType::Vec2) => vk::Format::R8G8_SINT,
        //                 (ComponentType::I16, DataType::Vec2) => vk::Format::R16G16_SINT,
        //                 (ComponentType::U8, DataType::Vec2) => vk::Format::R8G8_UINT,
        //                 (ComponentType::U16, DataType::Vec2) => vk::Format::R16G16_UINT,
        //                 (ComponentType::U32, DataType::Vec2) => vk::Format::R32G32_UINT,
        //                 (ComponentType::F32, DataType::Vec2) => vk::Format::R32G32_SFLOAT,

        //                 (ComponentType::I8, DataType::Vec3) => vk::Format::R8G8B8_SINT,
        //                 (ComponentType::I16, DataType::Vec3) => vk::Format::R16G16B16_SINT,
        //                 (ComponentType::U8, DataType::Vec3) => vk::Format::R8G8B8_UINT,
        //                 (ComponentType::U16, DataType::Vec3) => vk::Format::R16G16B16_UINT,
        //                 (ComponentType::U32, DataType::Vec3) => vk::Format::R32G32B32_UINT,
        //                 (ComponentType::F32, DataType::Vec3) => vk::Format::R32G32B32_SFLOAT,

        //                 (ComponentType::I8, DataType::Vec4) => vk::Format::R8G8B8A8_SINT,
        //                 (ComponentType::I16, DataType::Vec4) => vk::Format::R16G16B16A16_SINT,
        //                 (ComponentType::U8, DataType::Vec4) => vk::Format::R8G8B8A8_UINT,
        //                 (ComponentType::U16, DataType::Vec4) => vk::Format::R16G16B16A16_UINT,
        //                 (ComponentType::U32, DataType::Vec4) => vk::Format::R32G32B32A32_UINT,
        //                 (ComponentType::F32, DataType::Vec4) => vk::Format::R32G32B32A32_SFLOAT,

        //                 _ => panic!("vk::Format from ({:?}, {:?}) not supported!", comp_type, data_type),
        //         }
        // }

        fn vk_filter_from_mag_filter(mag_filter: MagFilter) -> vk::Filter {
                match mag_filter {
                        MagFilter::Nearest => vk::Filter::NEAREST,
                        MagFilter::Linear => vk::Filter::LINEAR,
                }
        }

        fn vk_filter_from_min_filter(min_filter: MinFilter) -> vk::Filter {
                match min_filter {
                        MinFilter::Nearest => vk::Filter::NEAREST,
                        MinFilter::NearestMipmapNearest => vk::Filter::NEAREST,
                        MinFilter::NearestMipmapLinear => vk::Filter::NEAREST,
                        MinFilter::Linear => vk::Filter::LINEAR,
                        MinFilter::LinearMipmapNearest => vk::Filter::LINEAR,
                        MinFilter::LinearMipmapLinear => vk::Filter::LINEAR,
                }
        }

        fn vk_sampler_mipmap_mode_from_min_filter(min_filter: MinFilter) -> vk::SamplerMipmapMode {
                match min_filter {
                        MinFilter::NearestMipmapLinear => vk::SamplerMipmapMode::LINEAR,
                        MinFilter::LinearMipmapLinear => vk::SamplerMipmapMode::LINEAR,
                        MinFilter::NearestMipmapNearest => vk::SamplerMipmapMode::NEAREST,
                        MinFilter::LinearMipmapNearest => vk::SamplerMipmapMode::NEAREST,
                        _ => vk::SamplerMipmapMode::LINEAR,
                }
        }

        fn vk_sampler_address_mode_from_wrapping_mode(wrapping_mode: WrappingMode) -> vk::SamplerAddressMode {
                match wrapping_mode {
                        WrappingMode::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        WrappingMode::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
                        WrappingMode::Repeat => vk::SamplerAddressMode::REPEAT,
                }
        }
}

trait VkIndex {
        fn index_type() -> vk::IndexType;
}

impl VkIndex for u16 {
        fn index_type() -> vk::IndexType {
                vk::IndexType::UINT16
        }
}

impl VkIndex for u32 {
        fn index_type() -> vk::IndexType {
                vk::IndexType::UINT32
        }
}

impl From<CullMode> for vk::CullModeFlags {
        fn from(mode: CullMode) -> Self {
                match mode {
                        CullMode::None => vk::CullModeFlags::NONE,
                        CullMode::Front => vk::CullModeFlags::FRONT,
                        CullMode::Back => vk::CullModeFlags::BACK,
                }
        }
}

struct VkAssetManagerFrameData {}

pub struct VkShaderResource {
        pub resource_type: VkShaderResourceType,
}

pub enum VkShaderResourceType {
        UniformBuffer,
        UniformBufferDynamic,
        CombinedImageSampler,
}

impl VkShaderResourceType {
        fn descriptor_type(&self) -> vk::DescriptorType {
                match self {
                        VkShaderResourceType::UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
                        VkShaderResourceType::UniformBufferDynamic => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                        VkShaderResourceType::CombinedImageSampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                }
        }
}

#[derive(Debug, Clone, Copy, Enum, Hash, PartialEq, Eq)]
pub enum VkDescriptorSetIndex {
        World,
        RenderPass,
        Material,
        Mesh,
}

impl VkDescriptorSetIndex {
        pub fn value(&self) -> u32 {
                match self {
                        VkDescriptorSetIndex::World => 0,
                        VkDescriptorSetIndex::RenderPass => 1,
                        VkDescriptorSetIndex::Material => 2,
                        VkDescriptorSetIndex::Mesh => 3,
                }
        }
}
