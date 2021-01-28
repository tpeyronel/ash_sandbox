use std::{error::Error, ffi::CString, mem::size_of, process::Command, slice, sync::Arc, time::Instant};

use ash::{prelude::VkResult, version::DeviceV1_0, vk, vk::CommandBuffer};
use imgui::DrawData;
use imgui_rs_vulkan_renderer::RendererVkContext;
use log::{error, trace};
use winit::window::Window;

use crate::{
        image::Image,
        my_vec::*,
        renderer::Renderer,
        vertex::Vertex,
        vk_buffer::{VkBuffer, VkBufferCreateInfo, VkImmutableBufferCreateInfo},
        vk_context::{VkContext, VkQueues, VkReusableCommandBuffer},
        vk_image::{VkImage, VkImageCreateInfo},
        vk_wrapper::{
                VkDescriptorSetLayout, VkDevice, VkImageView, VkPipeline, VkPipelineLayout, VkSampler, VkShaderModule,
        },
};

pub struct VkRenderer {
        window:     Arc<Window>,
        vk_context: VkContext,

        vertex_buffer:    VkBuffer,
        index_buffer:     VkBuffer,
        matrices_buffers: Vec<VkBuffer>,

        tex_vk_img:         VkImage,
        tex_vk_img_view:    VkImageView,
        tex_vk_img_sampler: VkSampler,

        desc_set_layout: VkDescriptorSetLayout,
        desc_sets:       Vec<vk::DescriptorSet>,

        graphics_pipeline_layout: VkPipelineLayout,
        graphics_pipeline:        VkPipeline,

        imgui_renderer: imgui_rs_vulkan_renderer::Renderer,

        creation_instant: Instant,
        frame_counter:    u32,
}

impl VkRenderer {
        pub fn new(window: &Arc<Window>, imgui_context: &mut imgui::Context) -> Result<Self, Box<dyn Error>> {
                let vk_context = VkContext::new(window, imgui_context)?;

                let vertex_buffer = Self::create_vertex_buffer(
                        &vk_context.device,
                        &vk_context.allocator,
                        &vk_context.queues,
                        &vk_context.setup_cmd_buffer,
                )?;
                trace!("Created vertex buffer");

                let index_buffer = Self::create_index_buffer(
                        &vk_context.device,
                        &vk_context.allocator,
                        &vk_context.queues,
                        &vk_context.setup_cmd_buffer,
                )?;
                trace!("Created index buffer");

                let matrices_buffers = Self::create_matrices_buffers(
                        &vk_context.device,
                        &vk_context.allocator,
                        vk_context.swapchain.img_count,
                )?;
                trace!("Created matrices uniform buffer");


                let (tex_vk_img, tex_vk_img_view, tex_vk_img_sampler) = Self::create_texture_image(
                        &vk_context.physical_device_limits,
                        &vk_context.device,
                        &vk_context.allocator,
                        &vk_context.setup_cmd_buffer,
                        vk_context.queues.graphics,
                )?;

                let desc_set_layout = Self::create_descriptor_set_layout(&vk_context.device)?;
                let desc_sets = Self::create_descriptor_sets(
                        &vk_context.device,
                        *vk_context.desc_pool,
                        *desc_set_layout,
                        &matrices_buffers,
                        *tex_vk_img_view,
                        *tex_vk_img_sampler,
                )?;
                trace!("Created VkDescriptorSets");

                let graphics_pipeline_layout =
                        Self::create_graphics_pipeline_layout(&vk_context.device, *desc_set_layout)?;
                trace!("Created VkGraphicsPipelineLayout");

                let graphics_pipeline = Self::create_graphics_pipeline(
                        &vk_context.device,
                        vk_context.swapchain.samples,
                        *vk_context.render_pass,
                        &graphics_pipeline_layout,
                )?;
                trace!("Created VkGraphicsPipeline");

                let imgui_renderer = imgui_rs_vulkan_renderer::Renderer::new(
                        &vk_context,
                        vk_context.swapchain.img_count as usize,
                        vk_context.swapchain.samples,
                        *vk_context.render_pass,
                        imgui_context,
                )?;

                Ok(Self {
                        window: Arc::clone(window),
                        vk_context,

                        vertex_buffer,
                        index_buffer,
                        matrices_buffers,

                        tex_vk_img,
                        tex_vk_img_view,
                        tex_vk_img_sampler,

                        desc_set_layout,
                        desc_sets,

                        graphics_pipeline_layout,
                        graphics_pipeline,

                        imgui_renderer,

                        creation_instant: Instant::now(),
                        frame_counter: 0,
                })
        }
}

impl Renderer for VkRenderer {
        fn draw(&mut self, imgui_draw_data: &DrawData) -> Result<(), Box<dyn Error>> {
                let on_render_pass_recreation = |vk_context: &VkContext,
                                                 render_pass: vk::RenderPass|
                 -> Result<(), Box<dyn Error>> {
                        self.imgui_renderer
                                .set_render_pass(vk_context, vk_context.swapchain.samples, render_pass)?;

                        self.graphics_pipeline = Self::create_graphics_pipeline(
                                &vk_context.device,
                                vk_context.swapchain.samples,
                                *vk_context.render_pass,
                                &self.graphics_pipeline_layout,
                        )?;

                        Ok(())
                };

                let (img_i, frame_i, draw_cmd_buffer) =
                        match unsafe { self.vk_context.begin_frame(on_render_pass_recreation)? } {
                                Some(v) => v,
                                None => return Ok(()),
                        };

                self.update_matrices_buffer(frame_i)?;

                unsafe {
                        self.vk_context.device.cmd_bind_pipeline(
                                draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *self.graphics_pipeline,
                        );

                        self.vk_context.device.cmd_set_viewport(
                                draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.vk_context.viewport),
                        );
                        self.vk_context.device.cmd_set_scissor(
                                draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.vk_context.scissor),
                        );

                        self.vk_context.device.cmd_bind_vertex_buffers(
                                draw_cmd_buffer,
                                0,
                                &[*self.vertex_buffer],
                                &[0],
                        );
                        self.vk_context.device.cmd_bind_index_buffer(
                                draw_cmd_buffer,
                                *self.index_buffer,
                                0,
                                vk::IndexType::UINT32,
                        );
                        self.vk_context.device.cmd_bind_descriptor_sets(
                                draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *self.graphics_pipeline_layout,
                                0,
                                &[self.desc_sets[img_i as usize]],
                                &[],
                        );

                        for _ in 0..1 {
                                self.vk_context.device.cmd_draw_indexed(draw_cmd_buffer, 36, 1, 0, 0, 0);
                        }

                        self.imgui_renderer
                                .cmd_draw(&self.vk_context, draw_cmd_buffer, imgui_draw_data)?;

                        self.vk_context.end_frame(img_i)?;
                }

                Ok(())
        }
}

impl VkRenderer {
        fn create_vertex_buffer(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkBuffer, Box<dyn Error>> {
                let data = [
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                ];

                let cinfo = VkImmutableBufferCreateInfo {
                        device,
                        allocator,
                        cmd_buffer: setup_cmd_buffer,
                        transfer_queue: queues.graphics,
                        buffer_usage: vk::BufferUsageFlags::VERTEX_BUFFER,
                        data: &data,
                };

                VkBuffer::new_immutable(&cinfo)
        }

        fn create_index_buffer(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkBuffer, Box<dyn Error>> {
                let data: [u32; 36] = [
                        0, 1, 2, 2, 3, 0, //
                        4, 5, 6, 6, 7, 4, //
                        8, 9, 10, 10, 11, 8, //
                        12, 13, 14, 14, 15, 12, //
                        16, 17, 18, 18, 19, 16, //
                        20, 21, 22, 22, 23, 20, //
                ];

                let cinfo = VkImmutableBufferCreateInfo {
                        device,
                        allocator,
                        cmd_buffer: setup_cmd_buffer,
                        transfer_queue: queues.graphics,
                        buffer_usage: vk::BufferUsageFlags::INDEX_BUFFER,
                        data: &data,
                };

                VkBuffer::new_immutable(&cinfo)
        }

        fn create_matrices_buffers(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                swch_img_count: u32,
        ) -> Result<Vec<VkBuffer>, Box<dyn Error>> {
                let buffer_size = std::mem::size_of::<Matrices3D>() as vk::DeviceSize;

                let cinfo = VkBufferCreateInfo {
                        device,
                        allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        alloc_flags: vma::AllocationCreateFlags::NONE,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                let mut buffers = Vec::with_capacity(swch_img_count as usize);

                for _ in 0..swch_img_count {
                        buffers.push(VkBuffer::new(&cinfo)?);
                }

                Ok(buffers)
        }

        fn create_texture_image(
                pd_limits: &vk::PhysicalDeviceLimits,
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                cmd_buffer: &VkReusableCommandBuffer,
                transfer_queue: vk::Queue,
        ) -> Result<(VkImage, VkImageView, VkSampler), Box<dyn Error>> {
                unsafe { stb_image::stb_image::bindgen::stbi_set_flip_vertically_on_load(1) };

                let img = Image::new(const_cstr!("res/tex/wall.jpg").as_cstr(), 4)?;

                let staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                device,
                                allocator,
                                buffer_size: img.data_size() as vk::DeviceSize,
                                buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                                mem_usage: vma::MemoryUsage::CpuToGpu,
                                alloc_flags: vma::AllocationCreateFlags::NONE,
                                req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                                pref_mem_flags: Default::default(),
                                mem_type_bits: 0,
                                q_family_indices: None,
                        };

                        VkBuffer::new(&buffer_cinfo)?
                };

                let buffer_data = staging_buffer.map_memory(&allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(img.data(), buffer_data, img.data_size());
                }
                staging_buffer.unmap_memory(&allocator)?;
                staging_buffer.flush_memory(&allocator)?;

                let vk_img_cinfo = VkImageCreateInfo {
                        image_type:           vk::ImageType::TYPE_2D,
                        format:               vk::Format::R8G8B8A8_SRGB,
                        extent:               vk::Extent3D {
                                width:  img.width(),
                                height: img.height(),
                                depth:  1,
                        },
                        mip_levels:           1,
                        array_layers:         1,
                        samples:              vk::SampleCountFlags::TYPE_1,
                        tiling:               vk::ImageTiling::OPTIMAL,
                        usage:                vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
                        queue_family_indices: None,
                        initial_layout:       vk::ImageLayout::UNDEFINED,
                        mem_usage:            vma::MemoryUsage::GpuOnly,
                        alloc_cflags:         vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags:       vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags:      Default::default(),
                };

                let vk_img = unsafe { VkImage::new(allocator, &vk_img_cinfo)? };

                cmd_buffer.record_and_submit(&device, transfer_queue, &[], &[], &[], |device, cmd_buffer| {
                        let barrier = vk::ImageMemoryBarrier {
                                src_access_mask: vk::AccessFlags::empty(),
                                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: *vk_img,
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                },
                                ..vk::ImageMemoryBarrier::default()
                        };

                        unsafe {
                                device.cmd_pipeline_barrier(
                                        cmd_buffer,
                                        vk::PipelineStageFlags::TOP_OF_PIPE,
                                        vk::PipelineStageFlags::TRANSFER,
                                        vk::DependencyFlags::empty(),
                                        &[],
                                        &[],
                                        &[barrier],
                                )
                        };

                        let region = vk::BufferImageCopy {
                                buffer_offset:       0,
                                buffer_row_length:   0,
                                buffer_image_height: 0,
                                image_subresource:   vk::ImageSubresourceLayers {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        mip_level:        0,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                },
                                image_offset:        vk::Offset3D {
                                        x: 0, y: 0, z: 0
                                },
                                image_extent:        vk::Extent3D {
                                        width:  img.width(),
                                        height: img.height(),
                                        depth:  1,
                                },
                        };

                        unsafe {
                                device.cmd_copy_buffer_to_image(
                                        cmd_buffer,
                                        *staging_buffer,
                                        *vk_img,
                                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        &[region],
                                )
                        };

                        let barrier = vk::ImageMemoryBarrier {
                                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                dst_access_mask: vk::AccessFlags::SHADER_READ,
                                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: *vk_img,
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                },
                                ..vk::ImageMemoryBarrier::default()
                        };

                        unsafe {
                                device.cmd_pipeline_barrier(
                                        cmd_buffer,
                                        vk::PipelineStageFlags::TRANSFER,
                                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                                        vk::DependencyFlags::empty(),
                                        &[],
                                        &[],
                                        &[barrier],
                                )
                        };

                        Ok(())
                })?;

                let vk_img_view = unsafe {
                        let vk_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *vk_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: vk::Format::R8G8B8A8_SRGB,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &vk_img_view_cinfo)?
                };

                cmd_buffer.wait(&device, u64::MAX)?;

                let vk_img_sampler = unsafe {
                        let sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: vk::Filter::LINEAR,
                                min_filter: vk::Filter::LINEAR,
                                address_mode_u: vk::SamplerAddressMode::REPEAT,
                                address_mode_v: vk::SamplerAddressMode::REPEAT,
                                address_mode_w: vk::SamplerAddressMode::REPEAT,
                                anisotropy_enable: vk::TRUE,
                                max_anisotropy: pd_limits.max_sampler_anisotropy,
                                compare_enable: 0,
                                compare_op: vk::CompareOp::ALWAYS,
                                mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                                mip_lod_bias: 0.0,
                                min_lod: 0.0,
                                max_lod: 0.0,
                                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                                unnormalized_coordinates: vk::FALSE,
                                ..vk::SamplerCreateInfo::default()
                        };

                        VkSampler::new(device, &sampler_cinfo)?
                };

                Ok((vk_img, vk_img_view, vk_img_sampler))
        }

        fn create_descriptor_set_layout(device: &Arc<VkDevice>) -> VkResult<VkDescriptorSetLayout> {
                let mat_binding = vk::DescriptorSetLayoutBinding {
                        binding:              0,
                        descriptor_type:      vk::DescriptorType::UNIFORM_BUFFER,
                        descriptor_count:     1,
                        stage_flags:          vk::ShaderStageFlags::VERTEX,
                        p_immutable_samplers: std::ptr::null(),
                };

                let tex_binding = vk::DescriptorSetLayoutBinding {
                        binding:              1,
                        descriptor_type:      vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                        descriptor_count:     1,
                        stage_flags:          vk::ShaderStageFlags::FRAGMENT,
                        p_immutable_samplers: std::ptr::null(),
                };

                let bindings = [mat_binding, tex_binding];

                let desc_set_layout_cinfo = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

                unsafe { VkDescriptorSetLayout::new(device, &desc_set_layout_cinfo) }
        }

        fn create_descriptor_sets(
                device: &ash::Device,
                desc_pool: vk::DescriptorPool,
                desc_layout: vk::DescriptorSetLayout,
                matrices_buffers: &[VkBuffer],
                img_view: vk::ImageView,
                sampler: vk::Sampler,
        ) -> VkResult<Vec<vk::DescriptorSet>> {
                let desc_set_layouts = vec![desc_layout; matrices_buffers.len()];

                let desc_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
                        .descriptor_pool(desc_pool)
                        .set_layouts(&desc_set_layouts);

                let desc_sets = unsafe { device.allocate_descriptor_sets(&desc_set_ainfo)? };

                assert_eq!(matrices_buffers.len(), desc_sets.len());

                for (matrices_buffer, &desc_set) in matrices_buffers.iter().zip(desc_sets.iter()) {
                        let buffer_info = vk::DescriptorBufferInfo {
                                buffer: **matrices_buffer,
                                offset: 0,
                                range:  size_of::<Matrices3D>() as vk::DeviceSize,
                        };

                        let mat_desc_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                                .dst_set(desc_set)
                                .dst_binding(0)
                                .dst_array_element(0)
                                .buffer_info(std::slice::from_ref(&buffer_info))
                                .build();

                        let sampler_info = vk::DescriptorImageInfo {
                                sampler,
                                image_view: img_view,
                                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        };

                        let sampler_desc_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                                .dst_set(desc_set)
                                .dst_binding(1)
                                .dst_array_element(0)
                                .image_info(std::slice::from_ref(&sampler_info))
                                .build();

                        let writes = [mat_desc_write, sampler_desc_write];

                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                }

                Ok(desc_sets)
        }

        fn create_graphics_pipeline_layout(
                device: &Arc<VkDevice>,
                desc_set_layout: vk::DescriptorSetLayout,
        ) -> VkResult<VkPipelineLayout> {
                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                        /*.push_constant_ranges(&[])*/
                        .set_layouts(std::slice::from_ref(&desc_set_layout));

                unsafe { VkPipelineLayout::new(device, &layout_cinfo) }
        }

        fn create_graphics_pipeline(
                device: &Arc<VkDevice>,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: &VkPipelineLayout,
        ) -> VkResult<VkPipeline> {
                let vert_shader = create_shader_module(device, "res/shader/basic_shader.vert")?;
                let frag_shader = create_shader_module(device, "res/shader/basic_shader.frag")?;

                let entry_point = CString::new("main").unwrap();

                let shader_stages = [
                        vk::PipelineShaderStageCreateInfo::builder()
                                .stage(vk::ShaderStageFlags::VERTEX)
                                .module(*vert_shader)
                                .name(&entry_point)
                                .build(),
                        vk::PipelineShaderStageCreateInfo::builder()
                                .stage(vk::ShaderStageFlags::FRAGMENT)
                                .module(*frag_shader)
                                .name(&entry_point)
                                .build(),
                ];

                let vert_binding_desc = [Vertex::vk_binding_description()];
                let vert_attrib_descs = Vertex::vk_attribute_descriptions();
                let vert_input_cinfo = vk::PipelineVertexInputStateCreateInfo::builder()
                        .vertex_binding_descriptions(&vert_binding_desc)
                        .vertex_attribute_descriptions(&vert_attrib_descs);

                let input_assembly_cinfo = vk::PipelineInputAssemblyStateCreateInfo::builder()
                        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
                        .primitive_restart_enable(false);

                let viewport = vk::Viewport {
                        x:         0.0,
                        y:         0.0,
                        width:     1.0,
                        height:    1.0,
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                let scissor = vk::Rect2D {
                        offset: vk::Offset2D {
                                x: 0, y: 0
                        },
                        extent: vk::Extent2D {
                                width: 1, height: 1
                        },
                };

                let viewport_state_cinfo = vk::PipelineViewportStateCreateInfo::builder()
                        .viewports(slice::from_ref(&viewport))
                        .scissors(slice::from_ref(&scissor));

                let rasterization_state_cinfo = vk::PipelineRasterizationStateCreateInfo::builder()
                        .depth_clamp_enable(false)
                        .rasterizer_discard_enable(false)
                        .polygon_mode(vk::PolygonMode::FILL)
                        .line_width(1.0)
                        .cull_mode(vk::CullModeFlags::NONE)
                        .front_face(vk::FrontFace::CLOCKWISE)
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
                        .color_write_mask(vk::ColorComponentFlags::all())
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
                        .layout(**pipeline_layout)
                        .render_pass(render_pass)
                        .subpass(0)
                        .build();

                unsafe { VkPipeline::new_graphics(device, vk::PipelineCache::null(), &graphics_pipeline_cinfo) }
        }

        fn update_matrices_buffer(&self, frame_i: usize) -> vma::Result<()> {
                let time = self.creation_instant.elapsed().as_secs_f32();

                let data = Matrices3D {
                        model: glm::rotate(&Mat4::identity(), time, &Vec3::new(0.0, 1.0, 0.0)),
                        view:  glm::look_at_lh(
                                &Vec3::new(0.0, time.sin(), -1.0),
                                &Vec3::new(0.0, 0.0, 0.0),
                                &Vec3::y(),
                        ),
                        proj:  glm::perspective_fov_lh_zo(
                                90.0f32.to_radians(),
                                self.window.inner_size().width as f32,
                                self.window.inner_size().height as f32,
                                0.1,
                                100.0,
                        ),
                };

                let buffer_size = std::mem::size_of::<Matrices3D>() as vk::DeviceSize;

                let buffer = &self.matrices_buffers[frame_i];
                let map = buffer.map_memory(&self.vk_context.allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(&data as *const _ as *const u8, map, buffer_size as usize);
                }
                buffer.unmap_memory(&self.vk_context.allocator)?;

                Ok(())
        }
}








struct Matrices3D {
        model: Mat4,
        view:  Mat4,
        proj:  Mat4,
}








fn create_shader_module(device: &Arc<VkDevice>, path: &'static str) -> VkResult<VkShaderModule> {
        let compile_path = path.to_string() + ".spv";

        let mut child = Command::new("res/misc/glslc.exe")
                .arg(path)
                .arg("-o")
                .arg(&compile_path)
                .spawn()
                .expect("Failed to compile shaders!");

        let exit_status = child.wait().expect("Error occurred while waiting for glslc.exe");

        if !exit_status.success() {
                error!(
                        "glslc.exe did not exit successfully: {}",
                        exit_status.code().unwrap_or(0)
                );
        }

        let shader_code = std::fs::read(&compile_path).expect("Failed to read shader binary file!");

        let mut shader_module_cinfo = vk::ShaderModuleCreateInfo::builder().build();
        shader_module_cinfo.code_size = shader_code.len();
        shader_module_cinfo.p_code = shader_code.as_ptr() as *const u32;

        assert_eq!(shader_code.len() % 4, 0, "Shader code is invalid!");

        unsafe { VkShaderModule::new(device, &shader_module_cinfo) }
}
