use std::{error::Error, ffi::CString, mem::size_of, process::Command, rc::Rc, slice, time::Instant};

use ash::{prelude::VkResult, version::DeviceV1_0, vk};
use imgui::DrawData;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use winit::window::Window;

use super::{
        vk_asset_manager::VkAssetManager,
        vk_buffer::{VkBuffer, VkBufferCreateInfo, VkImmutableBufferCreateInfo},
        vk_command_buffer::VkReusableCommandBuffer,
        vk_context::VkContext,
        vk_image::{VkImage, VkImageCreateFromDataInfo},
        vk_swapchain::{VkSwapchain, VkSwapchainOutdatedCauses},
        vk_wrapper::{
                VkDescriptorSetLayout, VkDevice, VkImageView, VkPipeline, VkPipelineLayout, VkRenderPass, VkSampler,
                VkSemaphore, VkShaderModule,
        },
};
use crate::{
        asset_manager::AssetManager,
        camera::Camera,
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE},
        image::Image2D,
        my_glm::*,
        renderer::Renderer,
        timer::Timer,
        vertex::Vertex,
        vk::{
                vk_buffer::BufferData,
                vk_image::MipLevels,
                vk_wrapper::{VkPhysicalDevice, VkQueues},
        },
};

pub struct VkRenderer {
        window: Rc<Window>,
        vk_context: VkContext,
        vk_asset_manager: VkAssetManager,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauses,

        render_pass: VkRenderPass,

        setup_cmd_buffer: VkReusableCommandBuffer,
        draw_cmd_buffers: Vec<VkReusableCommandBuffer>,

        vertex_buffer: VkBuffer,
        index_buffer: VkBuffer,
        matrices_buffers: Vec<VkBuffer>,

        tex_vk_img: VkImage,
        tex_vk_img_view: VkImageView,
        tex_vk_img_sampler: VkSampler,

        desc_set_layout: VkDescriptorSetLayout,
        desc_sets: Vec<vk::DescriptorSet>,

        graphics_pipeline_layout: VkPipelineLayout,
        graphics_pipeline: VkPipeline,

        imguir: imgui_rs_vulkan_renderer::Renderer,

        img_avail_semaphores: Vec<VkSemaphore>,
        present_complete_semaphores: Vec<VkSemaphore>,

        creation_instant: Instant,
        framei: usize,
        frame_counter: u32,
}

impl VkRenderer {
        pub fn new(
                window: &Rc<Window>,
                imguic: &mut imgui::Context,
                asset_manager: &AssetManager,
        ) -> Result<Self, Box<dyn Error>> {
                let vk_context = VkContext::new(window)?;
                let vk_asset_manager = VkAssetManager::new(
                        &vk_context.instance,
                        &vk_context.pdevice,
                        &vk_context.device,
                        &vk_context.allocator,
                        vk_context.queues.graphics,
                        &vk_context.cmd_pool,
                        asset_manager.buffers(),
                        asset_manager.buffer_views(),
                        asset_manager.images(),
                        asset_manager.samplers(),
                )?;

                let mut swapchain = VkSwapchain::new(
                        window,
                        &vk_context.instance,
                        &vk_context.surface,
                        *vk_context.pdevice,
                        &vk_context.device,
                        &vk_context.allocator,
                )?;
                trace!("Created VkSwapchain");

                let render_pass = Self::create_render_pass(
                        &vk_context.device,
                        swapchain.samples,
                        swapchain.color_format.format,
                        swapchain.depth_format,
                )?;
                trace!("Created VkRenderPass");

                swapchain.create_framebuffers(*render_pass)?;
                trace!("Created VkFramebuffers");

                let setup_cmd_buffer = VkReusableCommandBuffer::new(&vk_context.device, &vk_context.cmd_pool)?;
                let draw_cmd_buffers = VkReusableCommandBuffer::new_vec(
                        &vk_context.device,
                        &vk_context.cmd_pool,
                        swapchain.img_count,
                )?;
                trace!("Allocated VkCommandBuffers");

                let (img_avail_semaphores, present_complete_semaphores) =
                        Self::create_sync_objects(&vk_context.device, swapchain.img_count)?;
                trace!("Created VkSemaphores");

                let vertex_buffer = Self::create_vertex_buffer(
                        &vk_context.instance,
                        &vk_context.pdevice,
                        &vk_context.device,
                        &vk_context.allocator,
                        &vk_context.queues,
                        &setup_cmd_buffer,
                )?;
                trace!("Created vertex buffer");

                let index_buffer = Self::create_index_buffer(
                        &vk_context.device,
                        &vk_context.allocator,
                        &vk_context.queues,
                        &setup_cmd_buffer,
                )?;
                trace!("Created index buffer");

                let matrices_buffers =
                        Self::create_matrices_buffers(&vk_context.device, &vk_context.allocator, swapchain.img_count)?;
                trace!("Created matrices uniform buffer");

                let (tex_vk_img, tex_vk_img_view, tex_vk_img_sampler) = Self::create_texture_image(
                        &vk_context.instance,
                        &vk_context.pdevice,
                        &vk_context.pdevice.props.limits,
                        &vk_context.device,
                        &vk_context.allocator,
                        &setup_cmd_buffer,
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
                        swapchain.samples,
                        *render_pass,
                        &graphics_pipeline_layout,
                )?;
                trace!("Created VkGraphicsPipeline");

                let imgui_renderer = imgui_rs_vulkan_renderer::Renderer::new(
                        &vk_context,
                        swapchain.img_count as usize,
                        swapchain.samples,
                        *render_pass,
                        imguic,
                )?;

                Ok(Self {
                        window: Rc::clone(window),
                        vk_context,
                        vk_asset_manager,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauses::NONE,

                        render_pass,

                        setup_cmd_buffer,
                        draw_cmd_buffers,

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

                        imguir: imgui_renderer,

                        img_avail_semaphores,
                        present_complete_semaphores,

                        creation_instant: Instant::now(),

                        framei: 0,
                        frame_counter: 0,
                })
        }
}

impl Renderer for VkRenderer {
        fn draw(&mut self, cam: &Camera, imgui_draw_data: &DrawData) -> Result<(), Box<dyn Error>> {
                let (imgi, frame_i, draw_cmd_buffer) = match unsafe { self.begin_frame()? } {
                        Some(v) => v,
                        None => return Ok(()),
                };

                self.update_matrices_buffer(cam, frame_i)?;

                unsafe {
                        self.vk_context.device.cmd_bind_pipeline(
                                draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *self.graphics_pipeline,
                        );

                        self.vk_context.device.cmd_set_viewport(
                                draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.swapchain.viewport),
                        );
                        self.vk_context.device.cmd_set_scissor(
                                draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.swapchain.scissor),
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
                                &[self.desc_sets[imgi as usize]],
                                &[],
                        );

                        for _ in 0..1 {
                                self.vk_context.device.cmd_draw_indexed(draw_cmd_buffer, 36, 1, 0, 0, 0);
                        }

                        self.imguir
                                .cmd_draw(&self.vk_context, draw_cmd_buffer, imgui_draw_data)?;

                        self.end_frame(imgi)?;
                }

                Ok(())
        }

        fn on_window_resize(&mut self, _width: u32, _height: u32) {
                self.swapchain_outdated_causes
                        .insert(VkSwapchainOutdatedCauses::WINDOW_RESIZE);
        }
}

impl VkRenderer {
        fn recreate_swapchain_maybe(&mut self) -> Result<(), Box<dyn Error>> {
                match self.swapchain_outdated_causes {
                        VkSwapchainOutdatedCauses::NONE => return Ok(()),
                        VkSwapchainOutdatedCauses::WINDOW_RESIZE => {
                                // If resize is the only cause, then check that we actually need to resize
                                let wsize = &self.window.inner_size();
                                let ssize = &self.swapchain.extent;

                                if (wsize.width == ssize.width) && (wsize.height == ssize.height) {
                                        self.swapchain_outdated_causes = VkSwapchainOutdatedCauses::NONE;

                                        return Ok(());
                                }
                        }
                        _ => (),
                };

                trace!("Recreating VkSwapchain...");
                timer!("Recreated VkSwapchain in: ");

                let mut recreate_render_pass: bool = false;
                let mut recreate_pipeline: bool = self
                        .swapchain_outdated_causes
                        .contains(VkSwapchainOutdatedCauses::OUT_OF_DATE);

                unsafe { self.vk_context.device.device_wait_idle()? };

                let srecreation_info = self.swapchain.recreate()?;

                if srecreation_info.color_format_changed || srecreation_info.samples_changed {
                        recreate_render_pass = true;
                }

                if recreate_render_pass {
                        trace!("Recreating VkRenderPass...");

                        self.render_pass = Self::create_render_pass(
                                &self.vk_context.device,
                                self.swapchain.samples,
                                self.swapchain.color_format.format,
                                self.swapchain.depth_format,
                        )?;

                        self.imguir
                                .set_render_pass(&self.vk_context, self.swapchain.samples, *self.render_pass)?;

                        recreate_pipeline = true;
                }

                self.swapchain.create_framebuffers(*self.render_pass)?;

                if srecreation_info.img_count_changed {
                        trace!("Recreating VkObjects that depend on VkSwapchain img count...");
                        warn!("VkSwapchain image count changed!");

                        self.matrices_buffers = Self::create_matrices_buffers(
                                &self.vk_context.device,
                                &self.vk_context.allocator,
                                self.swapchain.img_count,
                        )?;

                        unsafe {
                                self.vk_context
                                        .device
                                        .free_descriptor_sets(*self.vk_context.desc_pool, &self.desc_sets)
                        };

                        self.desc_sets = Self::create_descriptor_sets(
                                &self.vk_context.device,
                                *self.vk_context.desc_pool,
                                *self.desc_set_layout,
                                &self.matrices_buffers,
                                *self.tex_vk_img_view,
                                *self.tex_vk_img_sampler,
                        )?;

                        self.draw_cmd_buffers = VkReusableCommandBuffer::new_vec(
                                &self.vk_context.device,
                                &self.vk_context.cmd_pool,
                                self.swapchain.img_count,
                        )?;

                        let (img_avail_semaphores, present_complete_semaphores) =
                                Self::create_sync_objects(&self.vk_context.device, self.swapchain.img_count)?;
                        self.img_avail_semaphores = img_avail_semaphores;
                        self.present_complete_semaphores = present_complete_semaphores;

                        self.framei = 0;
                }

                if recreate_pipeline {
                        trace!("Recreating VkGraphicsPipeline...");
                        self.graphics_pipeline = Self::create_graphics_pipeline(
                                &self.vk_context.device,
                                self.swapchain.samples,
                                *self.render_pass,
                                &self.graphics_pipeline_layout,
                        )?;
                }

                self.swapchain_outdated_causes = VkSwapchainOutdatedCauses::NONE;

                Ok(())
        }

        fn create_render_pass(
                device: &Rc<VkDevice>,
                swch_samples: vk::SampleCountFlags,
                swch_color_format: vk::Format,
                swch_depth_format: vk::Format,
        ) -> VkResult<VkRenderPass> {
                let attachments = [
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swch_color_format,
                                samples: swch_samples,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swch_depth_format,
                                samples: swch_samples,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swch_color_format,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::DONT_CARE,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                        },
                ];

                let color_attachment_ref = vk::AttachmentReference {
                        attachment: 0,
                        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                };

                let depth_attachment_ref = vk::AttachmentReference {
                        attachment: 1,
                        layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                };

                let resolve_attachment_ref = vk::AttachmentReference {
                        attachment: 2,
                        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                };

                let subpass_descriptions = [vk::SubpassDescription::builder()
                        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                        .color_attachments(slice::from_ref(&color_attachment_ref))
                        .depth_stencil_attachment(&depth_attachment_ref)
                        .resolve_attachments(slice::from_ref(&resolve_attachment_ref))
                        //.input_attachments(&[])
                        //.preserve_attachments(&[])
                        .build()];

                let subpass_dependencies = [vk::SubpassDependency {
                        src_subpass: vk::SUBPASS_EXTERNAL,
                        dst_subpass: 0,
                        src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                        dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                        dependency_flags: vk::DependencyFlags::empty(),
                }];

                let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                        .attachments(&attachments)
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { VkRenderPass::new(device, &render_pass_cinfo) }
        }

        fn create_sync_objects(
                device: &Rc<VkDevice>,
                swch_img_count: u32,
        ) -> VkResult<(Vec<VkSemaphore>, Vec<VkSemaphore>)> {
                fn create_n_semaphores(
                        device: &Rc<VkDevice>,
                        semaphore_cinfo: &vk::SemaphoreCreateInfo,
                        n: usize,
                ) -> VkResult<Vec<VkSemaphore>> {
                        let mut semaphores = Vec::with_capacity(n);

                        for _ in 0..n {
                                semaphores.push(unsafe { VkSemaphore::new(device, semaphore_cinfo)? });
                        }

                        Ok(semaphores)
                }

                let semaphore_cinfo = vk::SemaphoreCreateInfo::builder().build();

                Ok((
                        create_n_semaphores(device, &semaphore_cinfo, swch_img_count as usize)?,
                        create_n_semaphores(device, &semaphore_cinfo, swch_img_count as usize)?,
                ))
        }

        fn create_vertex_buffer(
                _instance: &ash::Instance,
                _pdevice: &VkPhysicalDevice,
                device: &ash::Device,
                allocator: &Rc<vma::Allocator>,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkBuffer, Box<dyn Error>> {
                let data = {
                        [
                                Vertex {
                                        pos: Vec3::new(-0.5, -0.5, -0.5),
                                        tex_coord: Vec2::new(0.0, 1.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, 0.5, -0.5),
                                        tex_coord: Vec2::new(0.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, 0.5, -0.5),
                                        tex_coord: Vec2::new(1.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, -0.5, -0.5),
                                        tex_coord: Vec2::new(1.0, 1.0),
                                },
                                //
                                //
                                //
                                Vertex {
                                        pos: Vec3::new(0.5, -0.5, -0.5),
                                        tex_coord: Vec2::new(0.0, 1.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, 0.5, -0.5),
                                        tex_coord: Vec2::new(0.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, 0.5, 0.5),
                                        tex_coord: Vec2::new(1.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, -0.5, 0.5),
                                        tex_coord: Vec2::new(1.0, 1.0),
                                },
                                //
                                //
                                //
                                Vertex {
                                        pos: Vec3::new(0.5, -0.5, 0.5),
                                        tex_coord: Vec2::new(0.0, 1.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, 0.5, 0.5),
                                        tex_coord: Vec2::new(0.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, 0.5, 0.5),
                                        tex_coord: Vec2::new(1.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, -0.5, 0.5),
                                        tex_coord: Vec2::new(1.0, 1.0),
                                },
                                //
                                //
                                //
                                Vertex {
                                        pos: Vec3::new(-0.5, -0.5, 0.5),
                                        tex_coord: Vec2::new(0.0, 1.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, 0.5, 0.5),
                                        tex_coord: Vec2::new(0.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, 0.5, -0.5),
                                        tex_coord: Vec2::new(1.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, -0.5, -0.5),
                                        tex_coord: Vec2::new(1.0, 1.0),
                                },
                                //
                                //
                                //
                                Vertex {
                                        pos: Vec3::new(-0.5, 0.5, -0.5),
                                        tex_coord: Vec2::new(0.0, 1.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, 0.5, 0.5),
                                        tex_coord: Vec2::new(0.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, 0.5, 0.5),
                                        tex_coord: Vec2::new(1.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, 0.5, -0.5),
                                        tex_coord: Vec2::new(1.0, 1.0),
                                },
                                //
                                //
                                //
                                Vertex {
                                        pos: Vec3::new(0.5, -0.5, -0.5),
                                        tex_coord: Vec2::new(0.0, 1.0),
                                },
                                Vertex {
                                        pos: Vec3::new(0.5, -0.5, 0.5),
                                        tex_coord: Vec2::new(0.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, -0.5, 0.5),
                                        tex_coord: Vec2::new(1.0, 0.0),
                                },
                                Vertex {
                                        pos: Vec3::new(-0.5, -0.5, -0.5),
                                        tex_coord: Vec2::new(1.0, 1.0),
                                },
                        ]
                };

                let cinfo = VkImmutableBufferCreateInfo {
                        device,
                        allocator,
                        cmd_buffer: setup_cmd_buffer,
                        transfer_queue: queues.graphics,
                        buffer_usage: vk::BufferUsageFlags::VERTEX_BUFFER,
                        data: BufferData::FullSlice(&data),
                };

                VkBuffer::new_immutable(&cinfo)
        }

        fn create_index_buffer(
                device: &ash::Device,
                allocator: &Rc<vma::Allocator>,
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
                        data: BufferData::FullSlice(&data),
                };

                VkBuffer::new_immutable(&cinfo)
        }

        fn create_matrices_buffers(
                device: &ash::Device,
                allocator: &Rc<vma::Allocator>,
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
                instance: &ash::Instance,
                pdevice: &vk::PhysicalDevice,
                pd_limits: &vk::PhysicalDeviceLimits,
                device: &Rc<VkDevice>,
                allocator: &Rc<vma::Allocator>,
                setup_cmd_buffer: &VkReusableCommandBuffer,
                transfer_queue: vk::Queue,
        ) -> Result<(VkImage, VkImageView, VkSampler), Box<dyn Error>> {
                unsafe { stb_image::stb_image::bindgen::stbi_set_flip_vertically_on_load(1) };

                let image = Image2D::new(const_cstr!("res/tex/wall.jpg").as_cstr(), 4)?;

                let vk_img_cinfo = VkImageCreateFromDataInfo {
                        data: image.data(),
                        width: image.width(),
                        height: image.height(),
                        format: vk::Format::R8G8B8A8_SRGB,
                        mip_levels: MipLevels::Log2,
                        samples: vk::SampleCountFlags::TYPE_1,
                        setup_cmd_buffer,
                        transfer_queue,
                };

                let vk_img = unsafe { VkImage::from_data(instance, pdevice, device, allocator, &vk_img_cinfo)? };

                unsafe {
                        setup_cmd_buffer.wait(&device, u64::MAX)?;
                }

                let vk_img_view = unsafe {
                        let vk_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *vk_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: vk::Format::R8G8B8A8_SRGB,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: vk_img.mip_levels,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &vk_img_view_cinfo)?
                };

                let vk_img_sampler = unsafe {
                        let sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: vk::Filter::LINEAR,
                                min_filter: vk::Filter::LINEAR,
                                address_mode_u: vk::SamplerAddressMode::REPEAT,
                                address_mode_v: vk::SamplerAddressMode::REPEAT,
                                address_mode_w: vk::SamplerAddressMode::REPEAT,
                                anisotropy_enable: ENABLE_ANISOTROPY as vk::Bool32,
                                max_anisotropy: pd_limits.max_sampler_anisotropy,
                                compare_enable: 0,
                                compare_op: vk::CompareOp::ALWAYS,
                                mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                                mip_lod_bias: 0.0,
                                min_lod: 0.0,
                                max_lod: LOD_CLAMP_NONE,
                                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                                unnormalized_coordinates: vk::FALSE,
                                ..vk::SamplerCreateInfo::default()
                        };

                        VkSampler::new(device, &sampler_cinfo)?
                };

                Ok((vk_img, vk_img_view, vk_img_sampler))
        }

        fn create_descriptor_set_layout(device: &Rc<VkDevice>) -> VkResult<VkDescriptorSetLayout> {
                let mat_binding = vk::DescriptorSetLayoutBinding {
                        binding: 0,
                        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::VERTEX,
                        p_immutable_samplers: std::ptr::null(),
                };

                let tex_binding = vk::DescriptorSetLayoutBinding {
                        binding: 1,
                        descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
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
                                range: size_of::<Matrices3D>() as vk::DeviceSize,
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
                device: &Rc<VkDevice>,
                desc_set_layout: vk::DescriptorSetLayout,
        ) -> VkResult<VkPipelineLayout> {
                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                        /*.push_constant_ranges(&[])*/
                        .set_layouts(std::slice::from_ref(&desc_set_layout));

                unsafe { VkPipelineLayout::new(device, &layout_cinfo) }
        }

        fn create_graphics_pipeline(
                device: &Rc<VkDevice>,
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

        unsafe fn begin_frame(&mut self) -> Result<Option<(u32, usize, vk::CommandBuffer)>, Box<dyn Error>> {
                let wsize = self.window.inner_size();
                if (wsize.width == 0) || (wsize.height == 0) {
                        return Ok(None);
                }

                self.recreate_swapchain_maybe()?;

                let frame_img_avail_semaphore = &self.img_avail_semaphores[self.framei];

                let imgi = {
                        let (imgi, suboptimal) = self.swapchain.acquire_next_image(
                                u64::MAX,
                                **frame_img_avail_semaphore,
                                vk::Fence::null(),
                        )?;

                        if suboptimal {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauses::SUBOPTIMAL);
                        }

                        imgi
                };

                let frame_draw_cmd_buffer = &self.draw_cmd_buffers[self.framei];
                let frame_framebuffer = &self.swapchain.framebuffers[imgi as usize];

                let time = self.creation_instant.elapsed().as_secs_f32();
                let intensity = ((time.sin() + 1.0) / 2.0) * 0.05;

                let clear_values = [
                        vk::ClearValue {
                                color: vk::ClearColorValue {
                                        float32: [intensity, intensity, intensity, 1.0],
                                },
                        },
                        vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                        },
                ];

                self.vk_context
                        .device
                        .wait_for_fences(&[*frame_draw_cmd_buffer.fence], true, u64::MAX)?;

                self.vk_context.device.reset_fences(&[*frame_draw_cmd_buffer.fence])?;
                self.vk_context.device.reset_command_buffer(
                        **frame_draw_cmd_buffer,
                        vk::CommandBufferResetFlags::RELEASE_RESOURCES,
                )?;

                let cmd_buffer_binfo =
                        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                self.vk_context
                        .device
                        .begin_command_buffer(**frame_draw_cmd_buffer, &cmd_buffer_binfo)?;

                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                        .render_pass(*self.render_pass)
                        .framebuffer(**frame_framebuffer)
                        .render_area(self.swapchain.scissor)
                        .clear_values(&clear_values);

                self.vk_context.device.cmd_begin_render_pass(
                        **frame_draw_cmd_buffer,
                        &render_pass_binfo,
                        vk::SubpassContents::INLINE,
                );

                Ok(Some((imgi, self.framei, **frame_draw_cmd_buffer)))
        }

        unsafe fn end_frame(&mut self, img_i: u32) -> Result<(), Box<dyn Error>> {
                let frame_img_avail_semaphore = &self.img_avail_semaphores[self.framei];
                let frame_present_complete_semaphore = &self.present_complete_semaphores[self.framei];
                let frame_draw_cmd_buffer = &self.draw_cmd_buffers[self.framei];

                self.vk_context.device.cmd_end_render_pass(**frame_draw_cmd_buffer);
                self.vk_context.device.end_command_buffer(**frame_draw_cmd_buffer)?;

                let cmd_buffers = [**frame_draw_cmd_buffer];
                let wait_semaphores = &[**frame_img_avail_semaphore];
                let wait_stages = &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
                let signal_semaphores = &[**frame_present_complete_semaphore];

                let submit_info = vk::SubmitInfo::builder()
                        .command_buffers(&cmd_buffers)
                        .wait_semaphores(wait_semaphores)
                        .wait_dst_stage_mask(wait_stages)
                        .signal_semaphores(signal_semaphores);

                self.vk_context.device.queue_submit(
                        self.vk_context.queues.graphics,
                        &[submit_info.build()],
                        *frame_draw_cmd_buffer.fence,
                )?;

                match self.swapchain.queue_present(
                        self.vk_context.queues.present,
                        &vk::PresentInfoKHR::builder()
                                .wait_semaphores(&[**frame_present_complete_semaphore])
                                .swapchains(&[*self.swapchain])
                                .image_indices(&[img_i]),
                ) {
                        Ok(suboptimal) if suboptimal => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauses::SUBOPTIMAL);
                        }
                        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauses::OUT_OF_DATE);
                        }
                        Err(err) => return Err(err.into()),
                        _ => {}
                };

                self.framei = (self.framei + 1) % (self.swapchain.img_count as usize);
                self.frame_counter += 1;

                Ok(())
        }

        fn update_matrices_buffer(&self, cam: &Camera, frame_i: usize) -> vma::Result<()> {
                let time = self.creation_instant.elapsed().as_secs_f32();

                let model = glm::rotate(&Mat4::identity(), time, &Vec3::new(0.0, 1.0, 0.0));

                let data = Matrices3D {
                        model,
                        view: *cam.get_view(),
                        proj: *cam.get_proj(),
                        /* proj: glm::perspective_fov_zo(
                                150.0f32.to_radians(),
                                self.window.inner_size().width as f32,
                                self.window.inner_size().height as f32,
                                0.1,
                                100.0,
                        ), */
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

#[allow(dead_code)]
struct Matrices3D {
        model: Mat4,
        view: Mat4,
        proj: Mat4,
}

fn create_shader_module(device: &Rc<VkDevice>, path: &'static str) -> VkResult<VkShaderModule> {
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
