use std::{
        error::Error,
        ffi::CString,
        mem::size_of,
        process::Command,
        rc::Rc,
        slice,
        sync::{Arc, Mutex},
        time::Instant,
};

use ash::{prelude::VkResult, vk};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use winit::window::Window;

use super::{
        vk_asset_manager::{VkAssetManager, VkShader},
        vk_buffer::{VkBuffer, VkBufferCreateInfo},
        vk_command_buffer::VkReusableCommandBuffer,
        vk_context::VkContext,
        vk_swapchain::{VkSwapchain, VkSwapchainOutdatedCauses},
        vk_wrapper::{
                VkDescriptorSetLayout, VkDevice, VkPipeline, VkPipelineLayout, VkRenderPass, VkSemaphore,
                VkShaderModule,
        },
};
use crate::{scoped_timer::TimePrefix, asset_manager::ShaderId, renderer::ModelInstance};
use crate::{
        asset_manager::{AssetManager, Mesh, ModelId, Primitive},
        my_glm::*,
        render_state_switcher::RenderStateSwitcher,
        renderer::{RenderState, Renderer},
        vertex::Vertex,
};

pub struct VkRenderer {
        target_ticktime: f32,
        window: Rc<Window>,
        asset_manager: Arc<AssetManager>,

        vk_context: VkContext,
        vk_asset_manager: VkAssetManager,

        basic_shader_id: ShaderId,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauses,

        render_pass: VkRenderPass,

        _setup_cmd_buffer: VkReusableCommandBuffer,
        draw_cmd_buffers: Vec<VkReusableCommandBuffer>,

        matrices_buffers: Vec<VkBuffer>,
        lights_buffers: Vec<VkBuffer>,

        matrices_dst_set_layout: VkDescriptorSetLayout,
        lights_dst_set_layout: VkDescriptorSetLayout,
        // _material_dst_set_layout: VkDescriptorSetLayout,
        matrices_dst_sets: Vec<vk::DescriptorSet>,
        lights_dst_sets: Vec<vk::DescriptorSet>,

        graphics_pipeline_layout: VkPipelineLayout,
        graphics_pipeline: VkPipeline,

        imgui_renderer: imgui_rs_vulkan_renderer::Renderer,

        img_avail_semaphores: Vec<VkSemaphore>,
        present_complete_semaphores: Vec<VkSemaphore>,

        render_state_switcher: Arc<Mutex<RenderStateSwitcher>>,
        last_render_state_switch_timestamp: Instant,
        old_render_state: Option<Box<RenderState>>,
        new_render_state: Option<Box<RenderState>>,

        creation_instant: Instant,
        framei: usize,
        frame_counter: u32,
}

impl VkRenderer {
        pub fn new(
                target_tps: u32,
                window: Rc<Window>,
                imguic: &mut imgui::Context,
                asset_manager: Arc<AssetManager>,
                render_state_switcher: Arc<Mutex<RenderStateSwitcher>>,
        ) -> Result<Self, Box<dyn Error>> {
                let vk_context = VkContext::new(Rc::clone(&window))?;

                let mut swapchain = VkSwapchain::new(
                        Rc::clone(&window),
                        Rc::clone(&vk_context.instance),
                        Rc::clone(&vk_context.surface),
                        *vk_context.pdevice,
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.allocator),
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

                let setup_cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&vk_context.device), Rc::clone(&vk_context.cmd_pool))?;
                let draw_cmd_buffers = VkReusableCommandBuffer::new_vec(
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.cmd_pool),
                        swapchain.img_count,
                )?;
                trace!("Allocated VkCommandBuffers");

                let (img_avail_semaphores, present_complete_semaphores) =
                        Self::create_sync_objects(&vk_context.device, swapchain.img_count)?;
                trace!("Created VkSemaphores");

                let matrices_buffers = Self::create_matrices_buffers(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        swapchain.img_count,
                )?;
                trace!("Created matrices uniform buffer");

                let lights_buffers = Self::create_lights_buffers(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        swapchain.img_count,
                )?;
                trace!("Created lights uniform buffer");

                let (matrices_dst_set_layout, material_dst_set_layout, lights_dst_set_layout) =
                        Self::create_descriptor_set_layouts(&vk_context.device)?;

                let matrices_dst_sets = Self::create_matrices_dst_sets(
                        &vk_context.device,
                        *vk_context.dst_pool,
                        *matrices_dst_set_layout,
                        &matrices_buffers,
                )?;

                let lights_dst_sets = Self::create_lights_dst_sets(
                        &vk_context.device,
                        *vk_context.dst_pool,
                        *lights_dst_set_layout,
                        &lights_buffers,
                )?;

                trace!("Created VkDescriptorSets");

                let vk_asset_manager = VkAssetManager::new(
                        &vk_context.instance,
                        &vk_context.pdevice,
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.allocator),
                        vk_context.queues.graphics,
                        Rc::clone(&vk_context.cmd_pool),
                        *vk_context.dst_pool,
                        *material_dst_set_layout,
                        &asset_manager,
                )?;
                trace!("Created VkAssetManager");

                let basic_shader_id  = asset_manager.shader_names()["basic_shader"];

                let graphics_pipeline_layout = Self::create_graphics_pipeline_layout(
                        &vk_context.device,
                        &[
                                *matrices_dst_set_layout,
                                *material_dst_set_layout,
                                *lights_dst_set_layout,
                        ],
                )?;
                trace!("Created VkGraphicsPipelineLayout");

                let graphics_pipeline = Self::create_graphics_pipeline(
                        &vk_asset_manager.shaders[basic_shader_id],
                        &vk_context.device,
                        swapchain.samples,
                        *render_pass,
                        &graphics_pipeline_layout,
                )?;
                trace!("Created VkGraphicsPipeline");

                let imgui_renderer_options = imgui_rs_vulkan_renderer::Options {
                        in_flight_frames: swapchain.img_count as usize,
                        enable_depth_test: false,
                        enable_depth_write: false,
                        sample_count: swapchain.samples,
                };

                let imgui_renderer = imgui_rs_vulkan_renderer::Renderer::with_default_allocator(
                        &**vk_context.instance,
                        *vk_context.pdevice,
                        (**vk_context.device).clone(),
                        vk_context.queues.graphics,
                        **vk_context.cmd_pool,
                        *render_pass,
                        imguic,
                        Some(imgui_renderer_options),
                )?;

                Ok(Self {
                        target_ticktime: 1.0 / target_tps as f32,
                        window,
                        asset_manager,

                        vk_context,
                        vk_asset_manager,
                        basic_shader_id,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauses::NONE,

                        render_pass,

                        _setup_cmd_buffer: setup_cmd_buffer,
                        draw_cmd_buffers,

                        matrices_buffers,
                        lights_buffers,

                        matrices_dst_set_layout,
                        lights_dst_set_layout,
                        // _material_dst_set_layout: material_dst_set_layout,
                        matrices_dst_sets,
                        lights_dst_sets,

                        graphics_pipeline_layout,
                        graphics_pipeline,

                        imgui_renderer,

                        img_avail_semaphores,
                        present_complete_semaphores,

                        render_state_switcher,
                        last_render_state_switch_timestamp: Instant::now(),
                        old_render_state: None,
                        new_render_state: None,

                        creation_instant: Instant::now(),

                        framei: 0,
                        frame_counter: 0,
                })
        }
}

impl Renderer for VkRenderer {
        fn draw(
                &'_ mut self,
                player_orien: &UnitQuat,
                imgui_draw_data: &imgui::DrawData,
        ) -> Result<(), Box<dyn Error>> {
                {
                        let mut render_state_switcher = match self.render_state_switcher.lock() {
                                Ok(v) => v,
                                Err(_e) => panic!(),
                        };

                        if render_state_switcher.is_new_state_available() {
                                let new_render_state =
                                        render_state_switcher.read_new_render_state(self.old_render_state.take());
                                self.old_render_state = self.new_render_state.replace(new_render_state);
                                self.last_render_state_switch_timestamp = Instant::now();
                        }
                }

                if self.old_render_state.is_none() || self.new_render_state.is_none() {
                        return Ok(());
                }

                let tick_scalar = f32::clamp(
                        self.last_render_state_switch_timestamp.elapsed().as_secs_f32() / self.target_ticktime,
                        0.0,
                        1.0,
                );

                let (imagei, draw_cmd_buffer) = match unsafe { self.begin_frame()? } {
                        BeginFrameResult::Draw {
                                imagei,
                                draw_cmd_buffer,
                        } => (imagei, draw_cmd_buffer),
                        BeginFrameResult::Skip => return Ok(()),
                };

                let winit::dpi::PhysicalSize { width, height } = self.window.inner_size();
                let aspect_ratio = width as f32 / height as f32;

                let old_camera_pos = &self.old_render_state.as_ref().unwrap().camera_pos;
                let new_camera_pos = &self.new_render_state.as_ref().unwrap().camera_pos;
                let lerped_camera_pos = Vec3::lerp(old_camera_pos, new_camera_pos, tick_scalar);

                let inverted_view_mat = Mat4::new_translation(&lerped_camera_pos) * player_orien.to_homogeneous();

                let view_mat = inverted_view_mat
                        .try_inverse()
                        .expect("Couldn't invert camera ViewMatrix!");

                let proj_mat = self
                        .new_render_state
                        .as_ref()
                        .unwrap()
                        .proj_camera
                        .calc_proj_matrix(aspect_ratio);

                let mut matrices = MatricesVPN {
                        view: view_mat,
                        proj: proj_mat,
                        normal: Mat4::identity(),
                };

                Self::update_matrices_buffer(&self.matrices_buffers[self.framei], &matrices)?;

                let lights = UniformLights {
                        light_pos: view_mat * Vec4::new(1.0, 2.5, 0.0, 1.0),
                        light_color: Vec4::new(0.9, 1.0, 0.9, 1.0),
                };

                Self::update_lights_buffer(&self.lights_buffers[self.framei], &lights).unwrap();

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

                        let matrices_dst_set = self.matrices_dst_sets[imagei as usize];
                        let lights_dst_set = self.lights_dst_sets[imagei as usize];

                        for (model, new_instance) in &self.new_render_state.as_ref().unwrap().model_instances {
                                let old_instance = self
                                        .old_render_state
                                        .as_ref()
                                        .unwrap()
                                        .model_instances
                                        .get(model)
                                        .unwrap_or(new_instance);

                                let old_pos = &old_instance.pos;
                                let new_pos = &new_instance.pos;
                                let interpolated_pos = Vec3::lerp(old_pos, new_pos, tick_scalar);

                                let old_orien = &old_instance.orien;
                                let new_orien = &new_instance.orien;
                                let interpolated_orien = UnitQuat::nlerp(old_orien, new_orien, tick_scalar);

                                let transform =
                                        Mat4::new_translation(&interpolated_pos)
                                        * interpolated_orien.to_homogeneous()
                                        * Mat4::new_nonuniform_scaling(&new_instance.scale);

                                Self::draw_model(
                                        &self.vk_context.device,
                                        draw_cmd_buffer,
                                        matrices_dst_set,
                                        lights_dst_set,
                                        *self.graphics_pipeline_layout,
                                        &self.asset_manager,
                                        &self.vk_asset_manager,
                                        &self.matrices_buffers[self.framei],
                                        &mut matrices,
                                        new_instance.model_id,
                                        &transform,
                                )?;
                        }

                        self.imgui_renderer.cmd_draw(draw_cmd_buffer, imgui_draw_data)?;

                        self.end_frame(imagei)?;
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
                        },
                        _ => (),
                };

                trace!("Recreating VkSwapchain...");
                scoped_timer!("Recreated VkSwapchain in: ", TimePrefix::Base);

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

                        self.imgui_renderer.set_render_pass(*self.render_pass)?;

                        recreate_pipeline = true;
                }

                self.swapchain.create_framebuffers(*self.render_pass)?;

                if srecreation_info.img_count_changed {
                        trace!("Recreating VkObjects that depend on VkSwapchain img count...");
                        warn!("VkSwapchain image count changed!");

                        self.matrices_buffers = Self::create_matrices_buffers(
                                &self.vk_context.device,
                                Rc::clone(&self.vk_context.allocator),
                                self.swapchain.img_count,
                        )?;

                        self.lights_buffers = Self::create_lights_buffers(
                                &self.vk_context.device,
                                Rc::clone(&self.vk_context.allocator),
                                self.swapchain.img_count,
                        )?;

                        unsafe {
                                self.vk_context
                                        .device
                                        .free_descriptor_sets(*self.vk_context.dst_pool, &self.matrices_dst_sets)?
                        };

                        self.matrices_dst_sets = Self::create_matrices_dst_sets(
                                &self.vk_context.device,
                                *self.vk_context.dst_pool,
                                *self.matrices_dst_set_layout,
                                &self.matrices_buffers,
                        )?;

                        unsafe {
                                self.vk_context
                                        .device
                                        .free_descriptor_sets(*self.vk_context.dst_pool, &self.lights_dst_sets)?
                        };

                        self.lights_dst_sets = Self::create_lights_dst_sets(
                                &self.vk_context.device,
                                *self.vk_context.dst_pool,
                                *self.lights_dst_set_layout,
                                &self.lights_buffers,
                        )?;

                        self.draw_cmd_buffers = VkReusableCommandBuffer::new_vec(
                                Rc::clone(&self.vk_context.device),
                                Rc::clone(&self.vk_context.cmd_pool),
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
                                &self.vk_asset_manager.shaders[self.basic_shader_id],
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

        fn create_matrices_buffers(
                device: &ash::Device,
                allocator: Rc<vma::Allocator>,
                swch_img_count: u32,
        ) -> Result<Vec<VkBuffer>, Box<dyn Error>> {
                let buffer_size = std::mem::size_of::<MatricesVPN>() as vk::DeviceSize;

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
                        buffers.push(VkBuffer::new(cinfo.clone())?);
                }

                Ok(buffers)
        }

        fn create_lights_buffers(
                device: &ash::Device,
                allocator: Rc<vma::Allocator>,
                swch_img_count: u32,
        ) -> Result<Vec<VkBuffer>, Box<dyn Error>> {
                let buffer_size = std::mem::size_of::<UniformLights>() as vk::DeviceSize;

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
                        buffers.push(VkBuffer::new(cinfo.clone())?);
                }

                Ok(buffers)
        }

        fn create_descriptor_set_layouts(
                device: &Rc<VkDevice>,
        ) -> VkResult<(VkDescriptorSetLayout, VkDescriptorSetLayout, VkDescriptorSetLayout)> {
                let matrices_dst_set_layout = {
                        let mat_binding = vk::DescriptorSetLayoutBinding {
                                binding: 0,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::VERTEX,
                                p_immutable_samplers: std::ptr::null(),
                        };

                        let matrices_dst_set_layout_cinfo =
                                vk::DescriptorSetLayoutCreateInfo::builder().bindings(slice::from_ref(&mat_binding));

                        unsafe { VkDescriptorSetLayout::new(device, &matrices_dst_set_layout_cinfo)? }
                };

                let material_dst_set_layout = {
                        let tex_binding = vk::DescriptorSetLayoutBinding {
                                binding: 0,
                                descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        };

                        let sampler_binding = vk::DescriptorSetLayoutBinding {
                                binding: 1,
                                descriptor_type: vk::DescriptorType::SAMPLER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        };

                        let mat_bindings = [tex_binding, sampler_binding];
                        let mat_set_layout_cinfo = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&mat_bindings);

                        unsafe { VkDescriptorSetLayout::new(device, &mat_set_layout_cinfo)? }
                };

                let lights_dst_set_layout = {
                        let lights_binding = vk::DescriptorSetLayoutBinding {
                                binding: 0,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        };

                        let lights_dst_set_layout_cinfo =
                                vk::DescriptorSetLayoutCreateInfo::builder().bindings(slice::from_ref(&lights_binding));

                        unsafe { VkDescriptorSetLayout::new(device, &lights_dst_set_layout_cinfo)? }
                };

                Ok((matrices_dst_set_layout, material_dst_set_layout, lights_dst_set_layout))
        }

        fn create_matrices_dst_sets(
                device: &ash::Device,
                dst_pool: vk::DescriptorPool,
                matrices_dst_set_layout: vk::DescriptorSetLayout,
                matrices_buffers: &[VkBuffer],
        ) -> VkResult<Vec<vk::DescriptorSet>> {
                let dst_set_layouts = vec![matrices_dst_set_layout; matrices_buffers.len()];

                let dst_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
                        .descriptor_pool(dst_pool)
                        .set_layouts(&dst_set_layouts);

                let dst_sets = unsafe { device.allocate_descriptor_sets(&dst_set_ainfo)? };

                assert_eq!(matrices_buffers.len(), dst_sets.len());

                for (matrices_buffer, &dst_set) in matrices_buffers.iter().zip(dst_sets.iter()) {
                        let buffer_info = vk::DescriptorBufferInfo {
                                buffer: **matrices_buffer,
                                offset: 0,
                                range: size_of::<MatricesVPN>() as vk::DeviceSize,
                        };

                        let matrices_dst_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                                .dst_set(dst_set)
                                .dst_binding(0)
                                .dst_array_element(0)
                                .buffer_info(std::slice::from_ref(&buffer_info))
                                .build();

                        let writes = [matrices_dst_write];

                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                }

                Ok(dst_sets)
        }

        fn create_lights_dst_sets(
                device: &ash::Device,
                dst_pool: vk::DescriptorPool,
                lights_dst_set_layout: vk::DescriptorSetLayout,
                lights_buffers: &[VkBuffer],
        ) -> VkResult<Vec<vk::DescriptorSet>> {
                let dst_set_layouts = vec![lights_dst_set_layout; lights_buffers.len()];

                let dst_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
                        .descriptor_pool(dst_pool)
                        .set_layouts(&dst_set_layouts);

                let dst_sets = unsafe { device.allocate_descriptor_sets(&dst_set_ainfo)? };

                assert_eq!(lights_buffers.len(), dst_sets.len());

                for (lights_buffer, &dst_set) in lights_buffers.iter().zip(dst_sets.iter()) {
                        let buffer_info = vk::DescriptorBufferInfo {
                                buffer: **lights_buffer,
                                offset: 0,
                                range: size_of::<UniformLights>() as vk::DeviceSize,
                        };

                        let lights_dst_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                                .dst_set(dst_set)
                                .dst_binding(0)
                                .dst_array_element(0)
                                .buffer_info(std::slice::from_ref(&buffer_info))
                                .build();

                        unsafe { device.update_descriptor_sets(std::slice::from_ref(&lights_dst_write), &[]) };
                }

                Ok(dst_sets)
        }

        fn create_graphics_pipeline_layout(
                device: &Rc<VkDevice>,
                dst_set_layouts: &[vk::DescriptorSetLayout],
        ) -> VkResult<VkPipelineLayout> {
                let push_constant_range = vk::PushConstantRange {
                        stage_flags: vk::ShaderStageFlags::VERTEX,
                        offset: 0,
                        size: std::mem::size_of::<MatricesMMvp>() as u32,
                };

                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                        .push_constant_ranges(std::slice::from_ref(&push_constant_range))
                        .set_layouts(dst_set_layouts);

                unsafe { VkPipelineLayout::new(device, &layout_cinfo) }
        }

        fn create_graphics_pipeline(
                shader: &VkShader,
                device: &Rc<VkDevice>,
                swapchain_samples: vk::SampleCountFlags,
                render_pass: vk::RenderPass,
                pipeline_layout: &VkPipelineLayout,
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

                let vert_binding_desc = Vertex::vk_binding_description();
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
                        .layout(**pipeline_layout)
                        .render_pass(render_pass)
                        .subpass(0)
                        .build();

                unsafe { VkPipeline::new_graphics(device, vk::PipelineCache::null(), &graphics_pipeline_cinfo) }
        }

        // fn create_graphics_pipeline_for_shader(
        //         device: &Rc<VkDevice>,
        //         swapchain_samples: vk::SampleCountFlags,
        //         render_pass: vk::RenderPass,
        //         pipeline_layout: &VkPipelineLayout,
        //         shader: VkShader,
        // ) -> VkResult<(VkPipelineLayout, VkPipeline)> {
        //         let push_constant_range = vk::PushConstantRange {
        //                 stage_flags: vk::ShaderStageFlags::VERTEX,
        //                 offset: 0,
        //                 size: std::mem::size_of::<MatricesMMvp>() as u32,
        //         };

        //         let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
        //                 .push_constant_ranges(std::slice::from_ref(&push_constant_range))
        //                 .set_layouts(dst_set_layouts);

        //         let pipeline_layout = unsafe { VkPipelineLayout::new(device, &layout_cinfo)? };

        //         let entry_point = CString::new("main").unwrap();

        //         let shader_stages = [
        //                 vk::PipelineShaderStageCreateInfo::builder()
        //                         .stage(vk::ShaderStageFlags::VERTEX)
        //                         .module(*shader.vert_module)
        //                         .name(&entry_point)
        //                         .build(),
        //                 vk::PipelineShaderStageCreateInfo::builder()
        //                         .stage(vk::ShaderStageFlags::FRAGMENT)
        //                         .module(*shader.frag_module)
        //                         .name(&entry_point)
        //                         .build(),
        //         ];

        //         let vert_binding_desc = Vertex::vk_binding_description();
        //         let vert_attrib_descs = Vertex::vk_attribute_descriptions();
        //         let vert_input_cinfo = vk::PipelineVertexInputStateCreateInfo::builder()
        //                 .vertex_binding_descriptions(&vert_binding_desc)
        //                 .vertex_attribute_descriptions(&vert_attrib_descs);

        //         let input_assembly_cinfo = vk::PipelineInputAssemblyStateCreateInfo::builder()
        //                 .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        //                 .primitive_restart_enable(false);

        //         let viewport = vk::Viewport {
        //                 x: 0.0,
        //                 y: 0.0,
        //                 width: 1.0,
        //                 height: 1.0,
        //                 min_depth: 0.0,
        //                 max_depth: 1.0,
        //         };

        //         let scissor = vk::Rect2D {
        //                 offset: vk::Offset2D { x: 0, y: 0 },
        //                 extent: vk::Extent2D { width: 1, height: 1 },
        //         };

        //         let viewport_state_cinfo = vk::PipelineViewportStateCreateInfo::builder()
        //                 .viewports(slice::from_ref(&viewport))
        //                 .scissors(slice::from_ref(&scissor));

        //         let rasterization_state_cinfo = vk::PipelineRasterizationStateCreateInfo::builder()
        //                 .depth_clamp_enable(false)
        //                 .rasterizer_discard_enable(false)
        //                 .polygon_mode(vk::PolygonMode::FILL)
        //                 .line_width(1.0)
        //                 .cull_mode(vk::CullModeFlags::NONE)
        //                 .front_face(vk::FrontFace::CLOCKWISE)
        //                 .depth_bias_enable(false)
        //                 .depth_bias_constant_factor(0.0)
        //                 .depth_bias_clamp(0.0)
        //                 .depth_bias_slope_factor(0.0);

        //         let multisample_state_cinfo = vk::PipelineMultisampleStateCreateInfo::builder()
        //                 .rasterization_samples(swapchain_samples)
        //                 .sample_shading_enable(false);

        //         let depth_stencil_state_cinfo = vk::PipelineDepthStencilStateCreateInfo::builder()
        //                 .depth_test_enable(true)
        //                 .depth_write_enable(true)
        //                 .depth_compare_op(vk::CompareOp::LESS)
        //                 .depth_bounds_test_enable(false)
        //                 .stencil_test_enable(false)
        //                 .build();

        //         let color_blend_attachments = [vk::PipelineColorBlendAttachmentState::builder()
        //                 .color_write_mask(vk::ColorComponentFlags::RGBA)
        //                 .blend_enable(false)
        //                 .build()];

        //         let color_blend_state_cinfo = vk::PipelineColorBlendStateCreateInfo::builder()
        //                 .attachments(&color_blend_attachments)
        //                 .logic_op_enable(false);

        //         let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        //         let pipeline_dyn_state_cinfo =
        //                 vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dyn_states);

        //         let graphics_pipeline_cinfo = vk::GraphicsPipelineCreateInfo::builder()
        //                 .stages(&shader_stages)
        //                 .vertex_input_state(&vert_input_cinfo)
        //                 .input_assembly_state(&input_assembly_cinfo)
        //                 .viewport_state(&viewport_state_cinfo)
        //                 .rasterization_state(&rasterization_state_cinfo)
        //                 .multisample_state(&multisample_state_cinfo)
        //                 .depth_stencil_state(&depth_stencil_state_cinfo)
        //                 .color_blend_state(&color_blend_state_cinfo)
        //                 .dynamic_state(&pipeline_dyn_state_cinfo)
        //                 .layout(**pipeline_layout)
        //                 .render_pass(render_pass)
        //                 .subpass(0)
        //                 .build();

        //         unsafe { VkPipeline::new_graphics(device, vk::PipelineCache::null(), &graphics_pipeline_cinfo) }
        // }

        unsafe fn begin_frame(&mut self) -> Result<BeginFrameResult, Box<dyn Error>> {
                let wsize = self.window.inner_size();
                if (wsize.width == 0) || (wsize.height == 0) {
                        return Ok(BeginFrameResult::Skip);
                }

                self.recreate_swapchain_maybe()?;

                let frame_img_avail_semaphore = &self.img_avail_semaphores[self.framei];

                let imagei = {
                        let (imagei, suboptimal) = self.swapchain.acquire_next_image(
                                u64::MAX,
                                **frame_img_avail_semaphore,
                                vk::Fence::null(),
                        )?;

                        if suboptimal {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauses::SUBOPTIMAL);
                        }

                        imagei
                };

                let frame_draw_cmd_buffer = &self.draw_cmd_buffers[self.framei];
                let frame_framebuffer = &self.swapchain.framebuffers[imagei as usize];

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

                Ok(BeginFrameResult::Draw {
                        imagei,
                        draw_cmd_buffer: **frame_draw_cmd_buffer,
                })
        }

        unsafe fn end_frame(&mut self, imagei: u32) -> Result<(), Box<dyn Error>> {
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
                                .image_indices(&[imagei]),
                ) {
                        Ok(suboptimal) if suboptimal => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauses::SUBOPTIMAL);
                        },
                        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauses::OUT_OF_DATE);
                        },
                        Err(err) => return Err(err.into()),
                        _ => {},
                };

                self.framei = (self.framei + 1) % (self.swapchain.img_count as usize);
                self.frame_counter += 1;

                Ok(())
        }

        fn update_matrices_buffer(matrices_buffer: &VkBuffer, matrices: &MatricesVPN) -> vma::Result<()> {
                let buffer_size = std::mem::size_of::<MatricesVPN>() as vk::DeviceSize;
                let map = matrices_buffer.map_memory()?;
                unsafe {
                        std::ptr::copy_nonoverlapping(matrices as *const _ as *const u8, map, buffer_size as usize);
                }
                matrices_buffer.unmap_memory()?;

                Ok(())
        }

        fn update_lights_buffer(light_buffer: &VkBuffer, lights: &UniformLights) -> vma::Result<()> {
                let buffer_size = std::mem::size_of::<UniformLights>() as vk::DeviceSize;
                let map = light_buffer.map_memory().unwrap();
                unsafe {
                        std::ptr::copy_nonoverlapping(lights as *const _ as *const u8, map, buffer_size as usize);
                }
                light_buffer.unmap_memory().unwrap();

                Ok(())
        }

        fn draw_instance(
                instance: &ModelInstance,
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                matrices_dst_set: vk::DescriptorSet,
                lights_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                matrices_buffer: &VkBuffer,
                matrices: &mut MatricesVPN,
                model_id: ModelId,
                model_instance_transform: &Mat4,
        ) {

        }

        fn draw_model(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                matrices_dst_set: vk::DescriptorSet,
                lights_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                matrices_buffer: &VkBuffer,
                matrices: &mut MatricesVPN,
                model_id: ModelId,
                model_instance_transform: &Mat4,
        ) -> vma::Result<()> {
                let model = &asset_manager.models()[model_id];

                let transform_final = model_instance_transform * model.base_transform;

                let mats_m_mvp = MatricesMMvp {
                        model: transform_final,
                        mvp: matrices.proj * matrices.view * transform_final,
                };

                unsafe {
                        device.cmd_push_constants(
                                draw_cmd_buffer,
                                pipeline_layout,
                                vk::ShaderStageFlags::VERTEX,
                                0,
                                slice::from_raw_parts(
                                        &mats_m_mvp as *const _ as *const u8,
                                        std::mem::size_of::<MatricesMMvp>(),
                                ),
                        );
                }

                matrices.normal = glm::inverse_transpose(transform_final);
                // Self::update_matrices_buffer(matrices_buffer, matrices)?;

                //let mut last_material = MaterialID::MAX;
                if let Some(mesh) = model.mesh {
                        Self::draw_mesh(
                                device,
                                draw_cmd_buffer,
                                matrices_dst_set,
                                lights_dst_set,
                                pipeline_layout,
                                vk_asset_manager,
                                &asset_manager.meshes()[mesh],
                        );
                }

                for &child in &model.children {
                        Self::draw_model(
                                device,
                                draw_cmd_buffer,
                                matrices_dst_set,
                                lights_dst_set,
                                pipeline_layout,
                                asset_manager,
                                vk_asset_manager,
                                matrices_buffer,
                                matrices,
                                child,
                                model_instance_transform,
                        )?;
                }

                Ok(())
        }

        fn draw_mesh(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                matrices_dst_set: vk::DescriptorSet,
                lights_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                vk_asset_manager: &VkAssetManager,
                mesh: &Mesh,
        ) {
                for primitive in &mesh.primitives {
                        Self::draw_primitive(
                                device,
                                draw_cmd_buffer,
                                matrices_dst_set,
                                lights_dst_set,
                                pipeline_layout,
                                vk_asset_manager,
                                primitive,
                        );
                }
        }

        fn draw_primitive(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                matrices_dst_set: vk::DescriptorSet,
                lights_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                vk_asset_manager: &VkAssetManager,
                primitive: &Primitive,
        ) {
                /* if primitive.material != last_material {
                        last_material = mesh.material;
                } */

                let material_dst_set = vk_asset_manager.material_dst_sets[primitive.material];
                let positions = &vk_asset_manager.buffer_views[primitive.positions];
                let normals = &vk_asset_manager.buffer_views[primitive.normals];
                let tex_coords = &vk_asset_manager.buffer_views[primitive.tex_coords];
                let indices = &vk_asset_manager.buffer_views[primitive.indices];

                unsafe {
                        device.cmd_bind_descriptor_sets(
                                draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                pipeline_layout,
                                0,
                                &[matrices_dst_set, material_dst_set, lights_dst_set],
                                &[],
                        );
                        device.cmd_bind_vertex_buffers(
                                draw_cmd_buffer,
                                0,
                                &[*positions.buffer, *normals.buffer, *tex_coords.buffer],
                                &[0, 0, 0],
                        );
                        device.cmd_bind_index_buffer(draw_cmd_buffer, *indices.buffer, 0, indices.index_type);

                        device.cmd_draw_indexed(draw_cmd_buffer, indices.element_count as u32, 1, 0, 0, 0);
                }
        }
}

impl Drop for VkRenderer {
        fn drop(&mut self) {
                let _ = unsafe { self.vk_context.device.device_wait_idle() };
        }
}

enum BeginFrameResult {
        Draw {
                imagei: u32,
                draw_cmd_buffer: vk::CommandBuffer,
        },
        Skip,
}

#[allow(dead_code)]
struct MatricesVPN {
        view: Mat4,
        proj: Mat4,
        normal: Mat4,
}

#[allow(dead_code)]
struct MatricesMMvp {
        model: Mat4,
        mvp: Mat4,
}

#[allow(dead_code)]
struct UniformLights {
        light_pos: Vec4,
        light_color: Vec4,
}
