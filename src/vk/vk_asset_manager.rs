use std::{ffi::CString, rc::Rc};

use ash::{prelude::VkResult, vk};
#[allow(unused_imports)]
use log::{debug, error, info, trace};
use slotmap::{SecondaryMap, SlotMap};

use crate::{
        asset_manager::{
                AssetManager, Buffer, BufferId, BufferView, BufferViewId, ComponentType, DataType, Image, ImageFormat,
                ImageId, MagFilter, Material, MaterialId, Mesh, MeshId, MinFilter, Sampler, SamplerId, Shader,
                ShaderId, ShaderResource, ShaderResourceId, Texture, TextureId, WrappingMode,
        },
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE},
        hashmap::HashMap,
        my_glm::{Vec2, Vec3},
        util::RefIntoSlice,
        vk::{
                vk_buffer::{BufferData, VkBuffer, VkImmutableBufferCreateInfo},
                vk_command_buffer::VkReusableCommandBuffer,
                vk_image::{MipLevels, VkImage, VkImageCreateFromDataInfo},
                vk_wrapper::{VkCommandPool, VkDevice, VkImageView, VkPhysicalDevice, VkSampler},
        },
        AnyResult,
};

use super::vk_wrapper::{VkDescriptorSetLayout, VkPipeline, VkShaderModule, VmaAllocator};

pub struct VkModelBufferView {
        pub buffer: VkBuffer,
        pub format: vk::Format,
        pub index_type: vk::IndexType,
        pub element_count: usize,
}

pub struct VkModelImage {
        pub image: VkImage,
        pub image_view: VkImageView,
}

pub struct VkShader {
        pub vert_module: VkShaderModule,
        pub frag_module: VkShaderModule,
        pub vertex_input_bindings: Vec<vk::VertexInputBindingDescription>,
        pub vertex_input_attributes: Vec<vk::VertexInputAttributeDescription>,
}

pub struct VkShaderResource {
        pub dst_set_layout: VkDescriptorSetLayout,
        pub descriptor_sets: Vec<vk::DescriptorSet>,
}

pub struct VkAssetManager {
        pub buffer_views: SecondaryMap<BufferViewId, VkModelBufferView>,
        pub images: SecondaryMap<ImageId, VkModelImage>,
        pub samplers: SecondaryMap<SamplerId, VkSampler>,
        pub material_dst_sets: SecondaryMap<MaterialId, vk::DescriptorSet>,
        pub shader_resources: HashMap<ShaderResourceId, VkShaderResource>,
        pub shaders: SecondaryMap<ShaderId, VkShader>,
        pub pipelines: SecondaryMap<ShaderId, VkPipeline>,
}

impl VkAssetManager {
        pub fn new(
                instance: &ash::Instance,
                pdevice: &VkPhysicalDevice,
                device: Rc<VkDevice>,
                allocator: Rc<VmaAllocator>,
                transfer_queue: vk::Queue,
                cmd_pool: Rc<VkCommandPool>,
                dst_pool: vk::DescriptorPool,
                material_dst_set_layout: vk::DescriptorSetLayout,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: vk::PipelineLayout,
                asset_manager: &AssetManager,
                frames_in_flight: usize,
        ) -> AnyResult<Self> {
                assert!(frames_in_flight > 0, "Frames in flight must be greater to zero");

                let cmd_buffer = VkReusableCommandBuffer::new(Rc::clone(&device), cmd_pool)?;

                trace!("Creating VkBuffers...");
                let vk_buffer_views = Self::create_vk_buffers_from_buffers(
                        &device,
                        Rc::clone(&allocator),
                        transfer_queue,
                        &cmd_buffer,
                        asset_manager.buffers(),
                        asset_manager.buffer_views(),
                        &Self::discover_buffer_view_usages(asset_manager.meshes()),
                )?;

                trace!("Creating VkImages...");
                let vk_images = Self::create_vk_images_from_images(
                        instance,
                        pdevice,
                        Rc::clone(&device),
                        allocator,
                        transfer_queue,
                        &cmd_buffer,
                        asset_manager.images(),
                )?;

                trace!("Creating VkSamplers...");
                let vk_samplers =
                        Self::create_vk_samplers_from_samplers(pdevice, Rc::clone(&device), asset_manager.samplers())?;

                trace!("Creating material VkDescriptorSets...");
                let vk_material_dst_sets = Self::create_vk_material_dst_sets_from_materials(
                        &device,
                        dst_pool,
                        material_dst_set_layout,
                        asset_manager.textures(),
                        asset_manager.materials(),
                        &vk_images,
                        &vk_samplers,
                )?;

                trace!("Creating VkShaderResources...");
                let vk_shader_resources = Self::create_vk_shaders_resources_from_shader_resources(
                        &device,
                        dst_pool,
                        asset_manager.shader_resources(),
                        frames_in_flight,
                )?;

                trace!("Creating VkShaders...");
                let vk_shaders = Self::create_vk_shaders_from_shaders(&device, asset_manager.shaders())?;

                trace!("Creating VkPipelines...");
                let vk_pipelines = Self::create_vk_pipelines_from_vk_shaders(
                        &device,
                        swapchain_samples,
                        render_pass,
                        pipeline_layout,
                        &vk_shaders,
                )?;

                unsafe { cmd_buffer.destroy() };

                Ok(Self {
                        buffer_views: vk_buffer_views,
                        images: vk_images,
                        samplers: vk_samplers,
                        material_dst_sets: vk_material_dst_sets,
                        shader_resources: vk_shader_resources,
                        shaders: vk_shaders,
                        pipelines: vk_pipelines,
                })
        }

        pub fn destroy(&mut self) {
                for (_, bv) in &self.buffer_views {
                        unsafe { bv.buffer.destroy() };
                }
                self.buffer_views.clear();

                for (_, i) in &self.images {
                        unsafe {
                                i.image.destroy();
                                i.image_view.destroy();
                        }
                }
                self.images.clear();

                for (_, s) in &self.samplers {
                        unsafe { s.destroy() };
                }
                self.samplers.clear();

                for (_, sr) in &self.shader_resources {
                        unsafe { sr.dst_set_layout.destroy() };
                }
                self.shader_resources.clear();

                for (_, s) in &self.shaders {
                        unsafe {
                                s.vert_module.destroy();
                                s.frag_module.destroy();
                        }
                }
                self.shaders.clear();

                self.pipelines.drain().for_each(|(_, pipeline)| {
                        unsafe { pipeline.destroy() };
                });
        }

        fn discover_buffer_view_usages(meshes: &SlotMap<MeshId, Mesh>) -> HashMap<BufferViewId, vk::BufferUsageFlags> {
                let mut vk_buffer_usages = HashMap::<BufferViewId, vk::BufferUsageFlags>::new();

                for (_, mesh) in meshes {
                        (*vk_buffer_usages.entry(mesh.positions).or_default()) |= vk::BufferUsageFlags::VERTEX_BUFFER;
                        (*vk_buffer_usages.entry(mesh.tex_coords).or_default()) |= vk::BufferUsageFlags::VERTEX_BUFFER;
                        (*vk_buffer_usages.entry(mesh.normals).or_default()) |= vk::BufferUsageFlags::VERTEX_BUFFER;
                        (*vk_buffer_usages.entry(mesh.tangents).or_default()) |= vk::BufferUsageFlags::VERTEX_BUFFER;
                        (*vk_buffer_usages.entry(mesh.indices).or_default()) |= vk::BufferUsageFlags::INDEX_BUFFER;
                }

                vk_buffer_usages
        }

        fn create_vk_buffers_from_buffers(
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                transfer_queue: vk::Queue,
                cmd_buffer: &VkReusableCommandBuffer,
                buffers: &SlotMap<BufferId, Buffer>,
                buffer_views: &SlotMap<BufferViewId, BufferView>,
                buffer_usages: &HashMap<BufferViewId, vk::BufferUsageFlags>,
        ) -> AnyResult<SecondaryMap<BufferViewId, VkModelBufferView>> {
                let mut vk_buffer_views = SecondaryMap::new();

                for (bview_id, bview) in buffer_views {
                        let buffer = &buffers[bview.buffer_id];

                        assert!(buffer.byte_length >= (bview.byte_offset + bview.byte_length));

                        let vk_buffer_cinfo = VkImmutableBufferCreateInfo {
                                device,
                                allocator: Rc::clone(&allocator),
                                cmd_buffer,
                                transfer_queue,
                                // TODO: accurate buffer usage flags
                                buffer_usage: buffer_usages[&bview_id],
                                data: BufferData::OffsetLength {
                                        data: buffer.bytes.as_slice(),
                                        offset: bview.byte_offset,
                                        length: bview.byte_length,
                                },
                        };

                        let vk_buffer = VkBuffer::new_immutable(vk_buffer_cinfo)?;

                        let format =
                                Self::vk_format_from_component_and_data_type(bview.component_type, bview.data_type);

                        let index_type = match bview.component_type {
                                ComponentType::U16 => vk::IndexType::UINT16,
                                ComponentType::U32 => vk::IndexType::UINT32,
                                _ => vk::IndexType::from_raw(i32::MAX),
                        };

                        vk_buffer_views.insert(
                                bview_id,
                                VkModelBufferView {
                                        buffer: vk_buffer,
                                        format,
                                        index_type,
                                        element_count: bview.element_count,
                                },
                        );
                }

                Ok(vk_buffer_views)
        }

        fn create_vk_images_from_images(
                instance: &ash::Instance,
                pdevice: &VkPhysicalDevice,
                device: Rc<VkDevice>,
                allocator: Rc<VmaAllocator>,
                transfer_queue: vk::Queue,
                cmd_buffer: &VkReusableCommandBuffer,
                images: &SlotMap<ImageId, Image>,
        ) -> AnyResult<SecondaryMap<ImageId, VkModelImage>> {
                let mut vk_images = SecondaryMap::new();

                for (image_id, image) in images {
                        let vk_image_cinfo = VkImageCreateFromDataInfo {
                                data: &image.pixels,
                                width: image.width,
                                height: image.height,
                                format: Self::vk_format_from_image_format(image.format),
                                mip_levels: MipLevels::Log2,
                                samples: vk::SampleCountFlags::TYPE_1,
                                setup_cmd_buffer: &cmd_buffer,
                                transfer_queue,
                        };

                        let vk_image = unsafe {
                                VkImage::from_data(instance, pdevice, &device, Rc::clone(&allocator), &vk_image_cinfo)?
                        };

                        let vk_image_view_cinfo = vk::ImageViewCreateInfo {
                                image: *vk_image,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: vk_image_cinfo.format,
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

                        let vk_image_view = unsafe { VkImageView::new(Rc::clone(&device), &vk_image_view_cinfo)? };

                        vk_images.insert(
                                image_id,
                                VkModelImage {
                                        image: vk_image,
                                        image_view: vk_image_view,
                                },
                        );
                }

                Ok(vk_images)
        }

        fn create_vk_samplers_from_samplers(
                pdevice: &VkPhysicalDevice,
                device: Rc<VkDevice>,
                samplers: &SlotMap<SamplerId, Sampler>,
        ) -> AnyResult<SecondaryMap<SamplerId, VkSampler>> {
                let mut vk_samplers = SecondaryMap::new();

                for (sampler_id, sampler) in samplers {
                        let vk_sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: Self::vk_filter_from_mag_filter(sampler.mag_filter),
                                min_filter: Self::vk_filter_from_min_filter(sampler.min_filter),
                                mipmap_mode: Self::vk_sampler_mipmap_mode_from_min_filter(sampler.min_filter),
                                address_mode_u: Self::vk_sampler_address_mode_from_wrapping_mode(sampler.wrap_s),
                                address_mode_v: Self::vk_sampler_address_mode_from_wrapping_mode(sampler.wrap_t),
                                address_mode_w: vk::SamplerAddressMode::REPEAT,
                                mip_lod_bias: 0.0,
                                anisotropy_enable: ENABLE_ANISOTROPY as vk::Bool32,
                                max_anisotropy: pdevice.max_sampler_anisotropy,
                                compare_enable: vk::FALSE,
                                compare_op: vk::CompareOp::ALWAYS,
                                min_lod: 0.0,
                                max_lod: LOD_CLAMP_NONE,
                                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                                unnormalized_coordinates: vk::FALSE,
                                ..Default::default()
                        };

                        vk_samplers.insert(sampler_id, unsafe {
                                VkSampler::new(Rc::clone(&device), &vk_sampler_cinfo)?
                        });
                }

                Ok(vk_samplers)
        }

        fn create_vk_material_dst_sets_from_materials(
                device: &VkDevice,
                dst_pool: vk::DescriptorPool,
                material_dst_set_layout: vk::DescriptorSetLayout,
                textures: &SlotMap<TextureId, Texture>,
                materials: &SlotMap<MaterialId, Material>,
                vk_images: &SecondaryMap<ImageId, VkModelImage>,
                vk_samplers: &SecondaryMap<SamplerId, VkSampler>,
        ) -> AnyResult<SecondaryMap<MaterialId, vk::DescriptorSet>> {
                let material_dst_set_layouts = vec![material_dst_set_layout; materials.len()];
                let dst_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
                        .descriptor_pool(dst_pool)
                        .set_layouts(&material_dst_set_layouts);
                let material_dst_sets = unsafe { device.allocate_descriptor_sets(&dst_set_ainfo)? };

                let mut material_dst_sets_map = SecondaryMap::new();

                for ((mat_id, mat), &material_dst_set) in materials.iter().zip(&material_dst_sets) {
                        material_dst_sets_map.insert(mat_id, material_dst_set);

                        let base_color_texture = match mat.base_color_texture {
                                Some(t) => t,
                                None => continue,
                        };

                        let color_texture = &textures[base_color_texture];
                        let color_vk_image_view = &vk_images[color_texture.image].image_view;
                        let color_vk_sampler = &vk_samplers[color_texture.sampler];

                        let image_info = vk::DescriptorImageInfo {
                                image_view: **color_vk_image_view,
                                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                ..Default::default()
                        };
                        let image_dst_set_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                                .dst_set(material_dst_set)
                                .dst_binding(0)
                                .dst_array_element(0)
                                .image_info(image_info.ref_into_slice())
                                .build();

                        let sampler_info = vk::DescriptorImageInfo {
                                sampler: **color_vk_sampler,
                                ..Default::default()
                        };
                        let sampler_dst_set_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::SAMPLER)
                                .dst_set(material_dst_set)
                                .dst_binding(1)
                                .dst_array_element(0)
                                .image_info(sampler_info.ref_into_slice())
                                .build();

                        unsafe { device.update_descriptor_sets(&[image_dst_set_write, sampler_dst_set_write], &[]) };
                }

                Ok(material_dst_sets_map)
        }

        fn create_vk_shaders_resources_from_shader_resources(
                device: &Rc<VkDevice>,
                dst_pool: vk::DescriptorPool,
                shader_resources: &HashMap<ShaderResourceId, ShaderResource>,
                frames_in_flight: usize,
        ) -> AnyResult<HashMap<ShaderResourceId, VkShaderResource>> {
                let mut vk_shader_resources = HashMap::<ShaderResourceId, VkShaderResource>::new();

                for (resource_id, shader_resource) in shader_resources {
                        let bindings: Vec<vk::DescriptorSetLayoutBinding> = shader_resource
                                .elements
                                .iter()
                                .enumerate()
                                .map(|(i, e)| vk::DescriptorSetLayoutBinding {
                                        binding: i as u32,
                                        descriptor_type: vk::DescriptorType::from(e.element_type),
                                        descriptor_count: 1,
                                        stage_flags: e.shader_stage_flags,
                                        p_immutable_samplers: std::ptr::null(),
                                })
                                .collect();

                        let matrices_dst_set_layout_cinfo =
                                vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

                        let dst_set_layout =
                                unsafe { VkDescriptorSetLayout::new(device, &matrices_dst_set_layout_cinfo)? };

                        let dst_set_layouts = vec![*dst_set_layout; frames_in_flight];

                        let dst_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
                                .descriptor_pool(dst_pool)
                                .set_layouts(&dst_set_layouts);

                        let descriptor_sets = unsafe { device.allocate_descriptor_sets(&dst_set_ainfo)? };

                        let vk_shader_resource = VkShaderResource {
                                dst_set_layout,
                                descriptor_sets,
                        };

                        vk_shader_resources.insert(resource_id.clone(), vk_shader_resource);
                }

                Ok(vk_shader_resources)
        }

        fn create_vk_shaders_from_shaders(
                device: &Rc<VkDevice>,
                shaders: &SlotMap<ShaderId, Shader>,
        ) -> AnyResult<SecondaryMap<ShaderId, VkShader>> {
                let mut vk_shaders = SecondaryMap::new();

                for (shader_id, shader) in shaders {
                        let vert_module = VkShaderModule::from_code(device, &shader.vert_module.bin)?;
                        let frag_module = VkShaderModule::from_code(device, &shader.frag_module.bin)?;

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

                        vk_shaders.insert(
                                shader_id,
                                VkShader {
                                        vert_module,
                                        frag_module,
                                        vertex_input_bindings,
                                        vertex_input_attributes,
                                },
                        );
                }

                Ok(vk_shaders)
        }

        fn create_vk_pipelines_from_vk_shaders(
                device: &Rc<VkDevice>,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: vk::PipelineLayout,
                vk_shaders: &SecondaryMap<ShaderId, VkShader>,
        ) -> AnyResult<SecondaryMap<ShaderId, VkPipeline>> {
                let mut vk_pipelines = SecondaryMap::new();

                for (shader_id, vk_shader) in vk_shaders {
                        vk_pipelines.insert(
                                shader_id,
                                Self::create_graphics_pipeline_from_vk_shader(
                                        device,
                                        swapchain_samples,
                                        render_pass,
                                        pipeline_layout,
                                        vk_shader,
                                )?,
                        );
                }

                Ok(vk_pipelines)
        }

        fn create_graphics_pipeline_from_vk_shader(
                device: &Rc<VkDevice>,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: vk::PipelineLayout,
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
                        .cull_mode(vk::CullModeFlags::BACK)
                        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                        .depth_bias_enable(false)
                        .depth_bias_constant_factor(0.0)
                        .depth_bias_clamp(0.0)
                        .depth_bias_slope_factor(0.0);

                let multisample_state_cinfo = vk::PipelineMultisampleStateCreateInfo::builder()
                        .rasterization_samples(swapchain_samples)
                        .sample_shading_enable(false);

                let depth_stencil_state_cinfo = vk::PipelineDepthStencilStateCreateInfo::builder()
                        .depth_test_enable(true)
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

        fn vk_format_from_component_and_data_type(comp_type: ComponentType, data_type: DataType) -> vk::Format {
                match (comp_type, data_type) {
                        (ComponentType::I8, DataType::Scalar) => vk::Format::R8_SINT,
                        (ComponentType::I16, DataType::Scalar) => vk::Format::R16_SINT,
                        (ComponentType::U8, DataType::Scalar) => vk::Format::R8_UINT,
                        (ComponentType::U16, DataType::Scalar) => vk::Format::R16_UINT,
                        (ComponentType::U32, DataType::Scalar) => vk::Format::R32_UINT,
                        (ComponentType::F32, DataType::Scalar) => vk::Format::R32_SFLOAT,

                        (ComponentType::I8, DataType::Vec2) => vk::Format::R8G8_SINT,
                        (ComponentType::I16, DataType::Vec2) => vk::Format::R16G16_SINT,
                        (ComponentType::U8, DataType::Vec2) => vk::Format::R8G8_UINT,
                        (ComponentType::U16, DataType::Vec2) => vk::Format::R16G16_UINT,
                        (ComponentType::U32, DataType::Vec2) => vk::Format::R32G32_UINT,
                        (ComponentType::F32, DataType::Vec2) => vk::Format::R32G32_SFLOAT,

                        (ComponentType::I8, DataType::Vec3) => vk::Format::R8G8B8_SINT,
                        (ComponentType::I16, DataType::Vec3) => vk::Format::R16G16B16_SINT,
                        (ComponentType::U8, DataType::Vec3) => vk::Format::R8G8B8_UINT,
                        (ComponentType::U16, DataType::Vec3) => vk::Format::R16G16B16_UINT,
                        (ComponentType::U32, DataType::Vec3) => vk::Format::R32G32B32_UINT,
                        (ComponentType::F32, DataType::Vec3) => vk::Format::R32G32B32_SFLOAT,

                        (ComponentType::I8, DataType::Vec4) => vk::Format::R8G8B8A8_SINT,
                        (ComponentType::I16, DataType::Vec4) => vk::Format::R16G16B16A16_SINT,
                        (ComponentType::U8, DataType::Vec4) => vk::Format::R8G8B8A8_UINT,
                        (ComponentType::U16, DataType::Vec4) => vk::Format::R16G16B16A16_UINT,
                        (ComponentType::U32, DataType::Vec4) => vk::Format::R32G32B32A32_UINT,
                        (ComponentType::F32, DataType::Vec4) => vk::Format::R32G32B32A32_SFLOAT,

                        _ => panic!("vk::Format from ({:?}, {:?}) not supported!", comp_type, data_type),
                }
        }

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
}
