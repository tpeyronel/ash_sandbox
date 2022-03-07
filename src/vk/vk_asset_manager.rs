use std::{ffi::CString, rc::Rc};

use ash::{
        prelude::VkResult,
        vk::{self, BufferUsageFlags},
};
#[allow(unused_imports)]
use log::{debug, error, info, trace};
use slotmap::SecondaryMap;

use crate::{
        asset_manager::{
                AssetManager, AssetManagerEvent, CubemapId, CullMode, ImageFormat, ImageId, IndicesVec, MagFilter,
                Material, MaterialId, MeshId, MinFilter, SamplerId, ShaderId, WrappingMode,
        },
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE},
        my_glm::{Vec2, Vec3},
        util::RefIntoSlice,
        vk::{
                vk_buffer::{BufferData, VkBuffer, VkImmutableBufferCreateInfo},
                vk_command_buffer::VkReusableCommandBuffer,
                vk_image::{MipLevels, VkImage, VkImageCreateFromDataInfo},
                vk_wrapper::{VkDevice, VkImageView, VkPhysicalDevice, VkSampler},
        },
        AnyResult,
};

use super::{
        vk_buffer::VkDynamicUniformBuffer,
        vk_context::VkContext,
        vk_descriptor_set_allocator::VkDescriptorSetAllocator,
        vk_descriptor_set_layout_cache::VkDescriptorSetLayoutCache,
        vk_image::VkImageCubemapCreateInfo,
        vk_renderer::MaterialData,
        vk_wrapper::{VkInstance, VkPipeline, VkPipelineLayout, VkShaderModule, VmaAllocator},
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
        pub dst_set: vk::DescriptorSet,
        pub material_data_buffer: VkDynamicUniformBuffer<MaterialData>,
}

pub struct VkShader {
        pub vert_module: VkShaderModule,
        pub frag_module: VkShaderModule,
        pub vertex_input_bindings: Vec<vk::VertexInputBindingDescription>,
        pub vertex_input_attributes: Vec<vk::VertexInputAttributeDescription>,
}

pub struct VkCubemap {
        pub image: VkImage,
        pub image_view: VkImageView,
        pub sampler: VkSampler,
}

// pub struct VkShaderResource {
//         pub dst_set_layout: VkDescriptorSetLayout,
//         pub descriptor_sets: Vec<vk::DescriptorSet>,
// }

pub struct VkAssetManager {
        instance: Rc<VkInstance>,
        pdevice: Rc<VkPhysicalDevice>,
        device: Rc<VkDevice>,
        allocator: Rc<VmaAllocator>,
        transfer_queue: vk::Queue,
        dst_set_allocator: VkDescriptorSetAllocator,
        cmd_buffer: VkReusableCommandBuffer,
        concurrent_frames: usize,

        material_dst_set_layout: vk::DescriptorSetLayout,
        pub graphics_pipeline_layout: VkPipelineLayout,

        swapchain_samples: vk::SampleCountFlags,
        render_pass: vk::RenderPass,

        pub meshes: SecondaryMap<MeshId, VkMesh>,
        pub images: SecondaryMap<ImageId, VkModelImage>,
        pub samplers: SecondaryMap<SamplerId, VkSampler>,
        pub materials: SecondaryMap<MaterialId, VkMaterial>,
        // pub shader_resources: HashMap<ShaderResourceId, VkShaderResource>,
        pub shaders: SecondaryMap<ShaderId, VkShader>,
        pub pipelines: SecondaryMap<ShaderId, VkPipeline>,
        pub cubemaps: SecondaryMap<CubemapId, VkCubemap>,
}

impl VkAssetManager {
        pub fn new(
                vk_context: &mut VkContext,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                world_dst_set_layout: vk::DescriptorSetLayout,
                object_dst_set_layout: vk::DescriptorSetLayout,
                concurrent_frames: usize,
        ) -> AnyResult<Self> {
                assert!(concurrent_frames > 0, "Frames in flight must be greater to zero");

                let dst_set_allocator = VkDescriptorSetAllocator::new(Rc::clone(&vk_context.device))?;
                let cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&vk_context.device), Rc::clone(&vk_context.cmd_pool))?;
                let material_dst_set_layout =
                        Self::create_material_dst_set_layout(&mut vk_context.dst_set_layout_cache)?;
                let graphics_pipeline_layout = Self::create_graphics_pipeline_layout(
                        &vk_context.device,
                        &[world_dst_set_layout, material_dst_set_layout, object_dst_set_layout],
                )?;

                // trace!("Creating VkShaderResources...");
                // let vk_shader_resources = Self::create_vk_shader_resources_from_shader_resources(
                //         &device,
                //         dst_pool,
                //         asset_manager.shader_resources(),
                //         frames_in_flight,
                // )?;

                Ok(Self {
                        instance: Rc::clone(&vk_context.instance),
                        pdevice: Rc::clone(&vk_context.pdevice),
                        device: Rc::clone(&vk_context.device),
                        allocator: Rc::clone(&vk_context.allocator),
                        transfer_queue: vk_context.queues.graphics,
                        dst_set_allocator,
                        cmd_buffer,
                        concurrent_frames,

                        material_dst_set_layout,

                        swapchain_samples,
                        render_pass,
                        graphics_pipeline_layout,

                        meshes: SecondaryMap::new(),
                        images: SecondaryMap::new(),
                        samplers: SecondaryMap::new(),
                        materials: SecondaryMap::new(),
                        // shader_resources: HashMap::new(),
                        shaders: SecondaryMap::new(),
                        pipelines: SecondaryMap::new(),
                        cubemaps: SecondaryMap::new(),
                })
        }

        pub fn process_asset_manager_events(&mut self, asset_manager: &AssetManager) -> AnyResult<()> {
                for e in asset_manager.events() {
                        match *e {
                                AssetManagerEvent::MeshUpdated(mesh_id) => {
                                        self.on_mesh_updated(asset_manager, mesh_id)?;
                                },
                                AssetManagerEvent::MeshDeleted(_) => todo!(),
                                AssetManagerEvent::ImageUpdated(image_id) => {
                                        self.on_image_updated(asset_manager, image_id)?;
                                },
                                AssetManagerEvent::ImageDeleted(_) => todo!(),
                                AssetManagerEvent::SamplerUpdated(sampler_id) => {
                                        self.on_sampler_updated(asset_manager, sampler_id)?;
                                },
                                AssetManagerEvent::SamplerDeleted(_) => todo!(),
                                AssetManagerEvent::MaterialUpdated(material_id) => {
                                        self.on_material_updated(asset_manager, material_id)?;
                                },
                                AssetManagerEvent::MaterialDeleted(_) => todo!(),
                                AssetManagerEvent::ShaderUpdated(shader_id) => {
                                        self.on_shader_updated(asset_manager, shader_id)?;
                                },
                                AssetManagerEvent::ShaderDeleted(_) => todo!(),
                                AssetManagerEvent::CubemapUpdated(cubemap_id) => {
                                        self.on_cubemap_updated(asset_manager, cubemap_id)?;
                                },
                                AssetManagerEvent::CubemapDeleted(_) => todo!(),
                                // AssetManagerEvent::TextureUpdated(texture_id) => todo!(),
                                // AssetManagerEvent::TextureDeleted(_) => todo!(),
                                // AssetManagerEvent::MeshUpdated(mid) => todo!(),
                                // AssetManagerEvent::MeshDeleted(_) => todo!(),
                                // AssetManagerEvent::ModelUpdated(mid) => todo!(),
                                // AssetManagerEvent::ModelDeleted(_) => todo!(),
                        }
                }

                Ok(())
        }

        pub fn destroy(&mut self) {
                self.cubemaps.drain().for_each(|(_, cubemap)| unsafe {
                        cubemap.image.destroy();
                        cubemap.image_view.destroy();
                        cubemap.sampler.destroy();
                });

                self.pipelines.drain().for_each(|(_, pipeline)| unsafe {
                        pipeline.destroy();
                });

                self.shaders.drain().for_each(|(_, shader)| unsafe {
                        shader.vert_module.destroy();
                        shader.frag_module.destroy();
                });

                self.materials.drain().for_each(|(_, material)| unsafe {
                        material.material_data_buffer.destroy();
                });

                // self.shader_resources.drain().for_each(|(_, shader_resource)| unsafe {
                //         shader_resource.dst_set_layout.destroy()
                // });

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

                unsafe { self.graphics_pipeline_layout.destroy() };
                unsafe { self.cmd_buffer.destroy() };
                unsafe { self.dst_set_allocator.destroy() };
        }

        fn create_material_dst_set_layout(
                dst_set_layout_cache: &mut VkDescriptorSetLayoutCache,
        ) -> VkResult<vk::DescriptorSetLayout> {
                let mat_data_binding = vk::DescriptorSetLayoutBinding {
                        binding: 0,
                        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
                        p_immutable_samplers: std::ptr::null(),
                };

                let diffuse_binding = vk::DescriptorSetLayoutBinding {
                        binding: 1,
                        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
                        p_immutable_samplers: std::ptr::null(),
                };

                let specular_binding = vk::DescriptorSetLayoutBinding {
                        binding: 2,
                        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
                        p_immutable_samplers: std::ptr::null(),
                };

                let sampler_binding = vk::DescriptorSetLayoutBinding {
                        binding: 3,
                        descriptor_type: vk::DescriptorType::SAMPLER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
                        p_immutable_samplers: std::ptr::null(),
                };

                unsafe {
                        dst_set_layout_cache.create_layout(vec![
                                mat_data_binding,
                                diffuse_binding,
                                specular_binding,
                                sampler_binding,
                        ])
                }
        }

        fn create_graphics_pipeline_layout(
                device: &Rc<VkDevice>,
                dst_set_layouts: &[vk::DescriptorSetLayout],
        ) -> VkResult<VkPipelineLayout> {
                // let push_constant_range = vk::PushConstantRange {
                //         stage_flags: vk::ShaderStageFlags::VERTEX,
                //         offset: 0,
                //         size: std::mem::size_of::<MatricesMMvp>() as u32,
                // };

                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                        // .push_constant_ranges(std::slice::from_ref(&push_constant_range))
                        .set_layouts(dst_set_layouts);

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
                } else {
                        if let Some(vk_image) = self.create_vk_image_from_image(asset_manager, image_id)? {
                                self.images.insert(image_id, vk_image);
                        }
                }

                Ok(())
        }

        fn on_sampler_updated(&mut self, asset_manager: &AssetManager, sampler_id: SamplerId) -> AnyResult<()> {
                if self.samplers.contains_key(sampler_id) {
                        todo!();
                } else {
                        if let Some(vk_sampler) = self.create_vk_sampler_from_sampler(asset_manager, sampler_id)? {
                                self.samplers.insert(sampler_id, vk_sampler);
                        }
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

        fn on_shader_updated(&mut self, asset_manager: &AssetManager, shader_id: ShaderId) -> AnyResult<()> {
                let shader = match asset_manager.shaders().get(shader_id) {
                        Some(shader) => shader,
                        None => return Ok(()),
                };

                let vert_module = VkShaderModule::from_code(&self.device, &shader.vert_module.bin)?;
                let frag_module = VkShaderModule::from_code(&self.device, &shader.frag_module.bin)?;

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
                                _ => panic!("Invalid shader vertex input: {}", vertex_input),
                        }

                        vertex_input_bindings.push(binding.build());
                        vertex_input_attributes.push(attribute.build());
                }

                let vk_shader = VkShader {
                        vert_module,
                        frag_module,
                        vertex_input_bindings,
                        vertex_input_attributes,
                };

                let vk_pipeline = Self::create_graphics_pipeline_from_vk_shader(
                        &self.device,
                        self.swapchain_samples,
                        self.render_pass,
                        *self.graphics_pipeline_layout,
                        !shader.disable_depth_test,
                        shader.cull_mode.into(),
                        &vk_shader,
                )?;

                self.shaders.insert(shader_id, vk_shader);
                self.pipelines.insert(shader_id, vk_pipeline);

                Ok(())
        }

        fn on_cubemap_updated(&mut self, asset_manager: &AssetManager, cubemap_id: CubemapId) -> AnyResult<()> {
                let cubemap = match asset_manager.cubemaps().get(cubemap_id) {
                        Some(cubemap) => cubemap,
                        None => return Ok(()),
                };

                let width = cubemap.faces[0].width;
                let height = cubemap.faces[0].width;

                for face in &cubemap.faces {
                        assert_eq!(width, face.width);
                        assert_eq!(height, face.height);
                }

                let faces_data = [
                        cubemap.faces[0].pixels.as_slice(),
                        cubemap.faces[1].pixels.as_slice(),
                        cubemap.faces[2].pixels.as_slice(),
                        cubemap.faces[3].pixels.as_slice(),
                        cubemap.faces[4].pixels.as_slice(),
                        cubemap.faces[5].pixels.as_slice(),
                ];

                let vk_cubemap_cinfo = VkImageCubemapCreateInfo {
                        faces_data,
                        width,
                        height,
                        format: vk::Format::R8G8B8A8_SRGB,
                        mip_levels: MipLevels::Log2,
                        samples: vk::SampleCountFlags::TYPE_1,
                        setup_cmd_buffer: &self.cmd_buffer,
                        transfer_queue: self.transfer_queue,
                };

                let vk_image = unsafe {
                        VkImage::new_cubemap(
                                &self.instance,
                                &self.pdevice,
                                &self.device,
                                Rc::clone(&self.allocator),
                                &vk_cubemap_cinfo,
                        )?
                };

                let vk_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *vk_image,
                        view_type: vk::ImageViewType::CUBE,
                        format: vk::Format::R8G8B8A8_SRGB, // vk_image_cinfo.format,
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

                let vk_cubemap = VkCubemap {
                        image: vk_image,
                        image_view: vk_image_view,
                        sampler: vk_sampler,
                };

                self.cubemaps.insert(cubemap_id, vk_cubemap);

                Ok(())
        }

        fn create_vk_mesh(&mut self, asset_manager: &AssetManager, mesh_id: MeshId) -> AnyResult<()> {
                let mesh = match asset_manager.meshes().get(mesh_id) {
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
                let image = match asset_manager.images().get(image_id) {
                        Some(image) => image,
                        None => return Ok(None),
                };

                let vk_image_cinfo = VkImageCreateFromDataInfo {
                        data: &image.pixels,
                        width: image.width,
                        height: image.height,
                        format: Self::vk_format_from_image_format(image.format),
                        mip_levels: MipLevels::Log2,
                        samples: vk::SampleCountFlags::TYPE_1,
                        setup_cmd_buffer: &self.cmd_buffer,
                        transfer_queue: self.transfer_queue,
                };

                let vk_image = unsafe {
                        VkImage::from_data(
                                &self.instance,
                                &self.pdevice,
                                &self.device,
                                Rc::clone(&self.allocator),
                                &vk_image_cinfo,
                        )?
                };

                let vk_image_view_cinfo = vk::ImageViewCreateInfo {
                        image: *vk_image,
                        view_type: vk::ImageViewType::TYPE_2D,
                        format: vk::Format::R8G8B8A8_SRGB, // vk_image_cinfo.format,
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
                let sampler = match asset_manager.samplers().get(sampler_id) {
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
                let material = match asset_manager.get_material(material_id) {
                        Some(material) => material,
                        None => return Ok(None),
                };

                let [material_dst_set] = unsafe {
                        self.dst_set_allocator
                                .allocate_descriptor_sets(&[self.material_dst_set_layout])?
                };

                let material_data_buffer = VkDynamicUniformBuffer::new(
                        &self.pdevice,
                        &self.device,
                        Rc::clone(&self.allocator),
                        self.concurrent_frames,
                )?;

                let buffer_info = vk::DescriptorBufferInfo {
                        buffer: *material_data_buffer,
                        offset: 0,
                        range: material_data_buffer.element_padded_size() as vk::DeviceSize,
                };

                let write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                        .dst_set(material_dst_set)
                        .dst_binding(0)
                        .dst_array_element(0)
                        .buffer_info(std::slice::from_ref(&buffer_info))
                        .build();

                unsafe { self.device.update_descriptor_sets(&[write], &[]) };

                self.update_vk_material(asset_manager, material, material_dst_set);

                let vk_material = VkMaterial {
                        dst_set: material_dst_set,
                        material_data_buffer,
                };

                Ok(Some(vk_material))
        }

        fn update_vk_material(
                &self,
                asset_manager: &AssetManager,
                material: &Material,
                material_dst_set: vk::DescriptorSet,
        ) {
                let base_color_texture = &asset_manager.textures()[material.base_color_texture];
                let metallic_roughness_texture = &asset_manager.textures()[material.metallic_roughness_texture];
                let diffuse_vk_image_view = &self.images[base_color_texture.image].image_view;
                let specular_vk_image_view = &self.images[metallic_roughness_texture.image].image_view;
                let color_vk_sampler = &self.samplers[base_color_texture.sampler];

                let diffuse_image_info = vk::DescriptorImageInfo {
                        image_view: **diffuse_vk_image_view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        ..Default::default()
                };
                let diffuse_image_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .dst_set(material_dst_set)
                        .dst_binding(1)
                        .dst_array_element(0)
                        .image_info(diffuse_image_info.ref_into_slice())
                        .build();

                let specular_image_info = vk::DescriptorImageInfo {
                        image_view: **specular_vk_image_view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        ..Default::default()
                };
                let specular_image_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .dst_set(material_dst_set)
                        .dst_binding(2)
                        .dst_array_element(0)
                        .image_info(specular_image_info.ref_into_slice())
                        .build();

                let sampler_info = vk::DescriptorImageInfo {
                        sampler: **color_vk_sampler,
                        ..Default::default()
                };
                let sampler_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .dst_set(material_dst_set)
                        .dst_binding(3)
                        .dst_array_element(0)
                        .image_info(sampler_info.ref_into_slice())
                        .build();

                unsafe {
                        self.device.update_descriptor_sets(
                                &[diffuse_image_write, specular_image_write, sampler_write],
                                &[],
                        )
                };
        }

        fn create_graphics_pipeline_from_vk_shader(
                device: &Rc<VkDevice>,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: vk::PipelineLayout,
                enable_depth_test: bool,
                cull_mode: vk::CullModeFlags,
                shader: &VkShader,
        ) -> VkResult<VkPipeline> {
                let entry_point = CString::new("main").unwrap();

                let shader_stages = [
                        vk::PipelineShaderStageCreateInfo::builder()
                                .stage(vk::ShaderStageFlags::VERTEX)
                                .module(*shader.vert_module)
                                .name(&entry_point)
                                .build(),
                        vk::PipelineShaderStageCreateInfo::builder()
                                .stage(vk::ShaderStageFlags::FRAGMENT)
                                .module(*shader.frag_module)
                                .name(&entry_point)
                                .build(),
                ];

                let vert_input_cinfo = vk::PipelineVertexInputStateCreateInfo::builder()
                        .vertex_binding_descriptions(&shader.vertex_input_bindings)
                        .vertex_attribute_descriptions(&shader.vertex_input_attributes);

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
                        .stencil_test_enable(false)
                        .build();

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
                        .subpass(0)
                        .build();

                unsafe { VkPipeline::new_graphics(device, vk::PipelineCache::null(), &graphics_pipeline_cinfo) }
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

        fn vk_format_from_image_format(img_format: ImageFormat) -> vk::Format {
                match img_format {
                        ImageFormat::R8 => vk::Format::R8_SRGB,
                        ImageFormat::R8G8 => vk::Format::R8G8_SRGB,
                        ImageFormat::R8G8B8 => vk::Format::R8G8B8_SRGB,
                        ImageFormat::R8G8B8A8 => vk::Format::R8G8B8A8_SRGB,
                        ImageFormat::B8G8R8 => vk::Format::B8G8R8_SRGB,
                        ImageFormat::B8G8R8A8 => vk::Format::B8G8R8A8_SRGB,
                        ImageFormat::R16 => vk::Format::R16_UINT,
                        ImageFormat::R16G16 => vk::Format::R16G16_UINT,
                        ImageFormat::R16G16B16 => vk::Format::R16G16B16_UINT,
                        ImageFormat::R16G16B16A16 => vk::Format::R16G16B16A16_UINT,
                }
        }

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

        // fn create_vk_shader_resources_from_shader_resources(
        //         device: &Rc<VkDevice>,
        //         dst_pool: vk::DescriptorPool,
        //         shader_resources: &HashMap<ShaderResourceId, ShaderResource>,
        //         frames_in_flight: usize,
        // ) -> AnyResult<HashMap<ShaderResourceId, VkShaderResource>> {
        //         let mut vk_shader_resources = HashMap::<ShaderResourceId, VkShaderResource>::new();

        //         for (resource_id, shader_resource) in shader_resources {
        //                 let bindings: Vec<vk::DescriptorSetLayoutBinding> = shader_resource
        //                         .elements
        //                         .iter()
        //                         .enumerate()
        //                         .map(|(i, e)| vk::DescriptorSetLayoutBinding {
        //                                 binding: i as u32,
        //                                 descriptor_type: vk::DescriptorType::from(e.element_type),
        //                                 descriptor_count: 1,
        //                                 stage_flags: e.shader_stage_flags,
        //                                 p_immutable_samplers: std::ptr::null(),
        //                         })
        //                         .collect();

        //                 let matrices_dst_set_layout_cinfo =
        //                         vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        //                 let dst_set_layout =
        //                         unsafe { VkDescriptorSetLayout::new(device, &matrices_dst_set_layout_cinfo)? };

        //                 let dst_set_layouts = vec![*dst_set_layout; frames_in_flight];

        //                 let dst_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
        //                         .descriptor_pool(dst_pool)
        //                         .set_layouts(&dst_set_layouts);

        //                 let descriptor_sets = unsafe { device.allocate_descriptor_sets(&dst_set_ainfo)? };

        //                 let vk_shader_resource = VkShaderResource {
        //                         dst_set_layout,
        //                         descriptor_sets,
        //                 };

        //                 vk_shader_resources.insert(resource_id.clone(), vk_shader_resource);
        //         }

        //         Ok(vk_shader_resources)
        // }
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
