use std::{ffi::CString, rc::Rc, slice, sync::Arc, time::Instant};

use ash::{prelude::VkResult, vk};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use winit::{dpi::PhysicalSize, window::Window};

use super::{
        vk_asset_manager::{VkAssetManager, VkShader},
        vk_buffer::VkBuffer,
        vk_command_buffer::VkReusableCommandBuffer,
        vk_context::VkContext,
        vk_swapchain::{VkSwapchain, VkSwapchainOutdatedCauseFlags},
        vk_wrapper::{VkDescriptorSetLayout, VkDevice, VkPipeline, VkPipelineLayout, VkRenderPass, VkSemaphore},
};
use crate::{asset_manager::ShaderId, constants::{MAX_CONCURRENT_FRAMES, DESIRED_SWAPCHAIN_IMG_COUNT}, scoped_timer::TimePrefix, AnyResult};
use crate::{
        asset_manager::{AssetManager, Mesh, ModelId, Primitive},
        my_glm::*,
        renderer::{RenderState, Renderer},
        vertex::Vertex,
};

struct VkFrameData {
        draw_cmd_buffer: VkReusableCommandBuffer,

        world_dst_set: vk::DescriptorSet,
        object_dst_set: vk::DescriptorSet,

        world_matrices_buffer: VkBuffer,
        world_light_buffer: VkBuffer,
        material_data_buffer: VkBuffer,
        object_matrices_buffer: VkBuffer,

        img_available_semaphore: VkSemaphore,
        present_complete_semaphore: VkSemaphore,
}

impl VkFrameData {
        fn new(
                vk_context: &VkContext,
                world_dst_set_layout: vk::DescriptorSetLayout,
                object_dst_set_layout: vk::DescriptorSetLayout,
        ) -> AnyResult<Self> {
                let draw_cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&vk_context.device), Rc::clone(&vk_context.cmd_pool))?;

                let [world_dst_set, object_dst_set] = unsafe {
                        vk_context.device.allocate_descriptor_sets_array(
                                *vk_context.dst_pool,
                                &[world_dst_set_layout, object_dst_set_layout],
                        )?
                };

                let world_matrices_buffer_size = std::mem::size_of::<WorldMatrices>() as vk::DeviceSize;
                let world_matrices_buffer = VkBuffer::new_uniform_buffer(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        std::mem::size_of::<WorldMatrices>() as vk::DeviceSize,
                )?;

                let world_light_buffer_size = std::mem::size_of::<WorldLight>() as vk::DeviceSize;
                let world_light_buffer = VkBuffer::new_uniform_buffer(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        world_light_buffer_size,
                )?;

                let material_data_buffer_size = std::mem::size_of::<MaterialData>() as vk::DeviceSize;
                let material_data_buffer = VkBuffer::new_uniform_buffer(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        material_data_buffer_size,
                )?;

                let object_matrices_padded_size = vk_context.pdevice.padded_size_of::<ObjectMatrices>();
                let object_matrices_buffer_size = object_matrices_padded_size as vk::DeviceSize * 2048;
                let object_matrices_buffer = VkBuffer::new_uniform_buffer(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        object_matrices_buffer_size,
                )?;

                let object_matrices = ObjectMatrices {
                        model: Mat4::new_translation(&Vec3::new(2.0, 0.0, 0.0)),
                        mvp: Mat4::identity(),
                        normal: Mat4::identity(),
                };
                object_matrices_buffer.write(&object_matrices)?;

                let world_matrices_buffer_info = vk::DescriptorBufferInfo {
                        buffer: *world_matrices_buffer,
                        offset: 0,
                        range: world_matrices_buffer_size,
                };

                let world_matrices_dst_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .dst_set(world_dst_set)
                        .dst_binding(0)
                        .dst_array_element(0)
                        .buffer_info(std::slice::from_ref(&world_matrices_buffer_info))
                        .build();

                let world_light_buffer_info = vk::DescriptorBufferInfo {
                        buffer: *world_light_buffer,
                        offset: 0,
                        range: world_light_buffer_size,
                };

                let world_light_dst_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .dst_set(world_dst_set)
                        .dst_binding(1)
                        .dst_array_element(0)
                        .buffer_info(std::slice::from_ref(&world_light_buffer_info))
                        .build();

                let object_matrices_buffer_info = vk::DescriptorBufferInfo {
                        buffer: *object_matrices_buffer,
                        offset: 0,
                        range: object_matrices_padded_size as vk::DeviceSize,
                };

                let object_matrices_dst_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                        .dst_set(object_dst_set)
                        .dst_binding(0)
                        .dst_array_element(0)
                        .buffer_info(std::slice::from_ref(&object_matrices_buffer_info))
                        .build();

                let writes = [
                        world_matrices_dst_write,
                        world_light_dst_write,
                        object_matrices_dst_write,
                ];
                unsafe { vk_context.device.update_descriptor_sets(&writes, &[]) };

                let semaphore_cinfo = vk::SemaphoreCreateInfo::builder().build();
                let img_available_semaphore = unsafe { VkSemaphore::new(&vk_context.device, &semaphore_cinfo)? };
                let present_complete_semaphore = unsafe { VkSemaphore::new(&vk_context.device, &semaphore_cinfo)? };

                Ok(Self {
                        draw_cmd_buffer,
                        world_dst_set,
                        object_dst_set,
                        world_matrices_buffer,
                        world_light_buffer,
                        material_data_buffer,
                        object_matrices_buffer,
                        img_available_semaphore,
                        present_complete_semaphore,
                })
        }
}

impl Drop for VkFrameData {
        fn drop(&mut self) {
                unsafe {
                        self.present_complete_semaphore.destroy();
                        self.img_available_semaphore.destroy();
                        self.object_matrices_buffer.destroy();
                        self.material_data_buffer.destroy();
                        self.world_light_buffer.destroy();
                        self.world_matrices_buffer.destroy();
                        self.draw_cmd_buffer.destroy();
                }
        }
}

pub struct VkRenderer {
        window: Rc<Window>,
        asset_manager: Arc<AssetManager>,

        vk_context: VkContext,
        vk_asset_manager: VkAssetManager,

        basic_shader_id: ShaderId,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags,

        render_pass: VkRenderPass,

        setup_cmd_buffer: VkReusableCommandBuffer,

        max_concurrent_frames: usize,
        frames_data: Vec<VkFrameData>,

        world_dst_set_layout: VkDescriptorSetLayout,
        material_dst_set_layout: VkDescriptorSetLayout,
        object_dst_set_layout: VkDescriptorSetLayout,

        graphics_pipeline_layout: VkPipelineLayout,
        graphics_pipeline: VkPipeline,

        imgui_renderer: Option<imgui_rs_vulkan_renderer::Renderer>,

        creation_instant: Instant,
        framei: usize,
}

impl VkRenderer {
        pub fn new(
                window: Rc<Window>,
                imguic: &mut imgui::Context,
                asset_manager: Arc<AssetManager>,
        ) -> AnyResult<Self> {
                let vk_context = VkContext::new(Rc::clone(&window))?;

                let mut swapchain = VkSwapchain::new(
                        Rc::clone(&window),
                        Rc::clone(&vk_context.instance),
                        Rc::clone(&vk_context.surface),
                        *vk_context.pdevice,
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.allocator),
                        DESIRED_SWAPCHAIN_IMG_COUNT,
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
                trace!("Allocated VkCommandBuffers");

                let world_dst_set_layout = Self::create_world_dst_set_layout(&vk_context.device)?;
                // let material_dst_set_layout = Self::create_material_dst_set_layout(&vk_context.device)?;
                let object_dst_set_layout = Self::create_object_dst_set_layout(&vk_context.device)?;

                let max_concurrent_frames = MAX_CONCURRENT_FRAMES;
                let frames_data = (0..max_concurrent_frames)
                        .map(|_| VkFrameData::new(&vk_context, *world_dst_set_layout, *object_dst_set_layout))
                        .collect::<AnyResult<Vec<VkFrameData>>>()?;

                let material_dst_set_layout = Self::create_descriptor_set_layouts(&vk_context.device)?;
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
                        swapchain.img_count as usize,
                )?;
                trace!("Created VkAssetManager");

                let basic_shader_id = asset_manager.shader_names()["basic-shader"];

                let graphics_pipeline_layout = Self::create_graphics_pipeline_layout(
                        &vk_context.device,
                        &[*world_dst_set_layout, *material_dst_set_layout, *object_dst_set_layout],
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
                        in_flight_frames: max_concurrent_frames,
                        enable_depth_test: false,
                        enable_depth_write: false,
                        sample_count: swapchain.samples,
                };

                let imgui_renderer = Some(imgui_rs_vulkan_renderer::Renderer::with_default_allocator(
                        &**vk_context.instance,
                        *vk_context.pdevice,
                        (**vk_context.device).clone(),
                        vk_context.queues.graphics,
                        **vk_context.cmd_pool,
                        *render_pass,
                        imguic,
                        Some(imgui_renderer_options),
                )?);

                Ok(Self {
                        window,
                        asset_manager,

                        vk_context,
                        vk_asset_manager,
                        basic_shader_id,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags::NONE,

                        render_pass,

                        setup_cmd_buffer,
                        max_concurrent_frames,
                        frames_data,

                        world_dst_set_layout,
                        material_dst_set_layout,
                        object_dst_set_layout,

                        graphics_pipeline_layout,
                        graphics_pipeline,

                        imgui_renderer,

                        creation_instant: Instant::now(),

                        framei: 0,
                })
        }
}

impl Renderer for VkRenderer {
        fn draw(
                &mut self,
                render_state: &RenderState,
                player_orien: &UnitQuat,
                imgui_draw_data: &imgui::DrawData,
        ) -> AnyResult<()> {
                if !self.should_render() {
                        return Ok(());
                }

                let imagei = match unsafe { self.begin_frame()? } {
                        BeginFrameResult::Draw { imagei } => imagei,
                        BeginFrameResult::Skip => return Ok(()),
                };

                let frame_data = &mut self.frames_data[self.framei];

                let PhysicalSize { width, height } = self.window.inner_size();
                let aspect_ratio = width as f32 / height as f32;

                let inverted_view_mat = Mat4::new_translation(&render_state.camera_pos) * player_orien.to_homogeneous();
                let view_mat = inverted_view_mat
                        .try_inverse()
                        .expect("Couldn't invert camera ViewMatrix!");

                let proj_mat = render_state.proj_camera.calc_proj_matrix(aspect_ratio);

                let world_matrices = WorldMatrices {
                        view: view_mat,
                        proj: proj_mat,
                };

                frame_data.world_matrices_buffer.write(&world_matrices)?;

                let (_, light) = render_state.lights.iter().next().unwrap();

                let world_light = WorldLight {
                        color: view_mat * Vec4::new_position(&light.0 .0),
                        pos: Vec4::new_position(&light.1 .0),
                };

                frame_data.world_light_buffer.write(&world_light)?;

                unsafe {
                        self.vk_context.device.cmd_bind_pipeline(
                                *frame_data.draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *self.graphics_pipeline,
                        );
                        self.vk_context.device.cmd_set_viewport(
                                *frame_data.draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.swapchain.viewport),
                        );
                        self.vk_context.device.cmd_set_scissor(
                                *frame_data.draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.swapchain.scissor),
                        );

                        self.vk_context.device.cmd_bind_descriptor_sets(
                                *frame_data.draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *self.graphics_pipeline_layout,
                                0,
                                &[frame_data.world_dst_set],
                                &[],
                        );

                        for (_, minstance) in &render_state.model_instances {
                                let transform = Mat4::new_translation(&minstance.pos)
                                        * UnitQuat::to_homogeneous(&minstance.orien)
                                        * Mat4::new_nonuniform_scaling(&minstance.scale);

                                Self::draw_model(
                                        &self.vk_context.device,
                                        *frame_data.draw_cmd_buffer,
                                        frame_data.object_dst_set,
                                        *self.graphics_pipeline_layout,
                                        &self.asset_manager,
                                        &self.vk_asset_manager,
                                        &world_matrices,
                                        minstance.model_id,
                                        &transform,
                                        0,
                                )?;
                        }

                        self.imgui_renderer
                                .as_mut()
                                .unwrap()
                                .cmd_draw(*frame_data.draw_cmd_buffer, imgui_draw_data)?;

                        self.end_frame(imagei)?;
                }

                Ok(())
        }

        fn on_window_resize(&mut self, _width: u32, _height: u32) {
                self.swapchain_outdated_causes
                        .insert(VkSwapchainOutdatedCauseFlags::WINDOW_RESIZE);
        }

        fn destroy(&mut self) -> AnyResult<()> {
                unsafe {
                        let _ = self.vk_context.device.device_wait_idle();
                        // self.present_complete_semaphores.drain(..).for_each(|s| s.destroy());
                        // self.img_avail_semaphores.drain(..).for_each(|s| s.destroy());
                        drop(self.imgui_renderer.take().unwrap());
                        self.graphics_pipeline.destroy();
                        self.graphics_pipeline_layout.destroy();
                        self.frames_data.clear();
                        self.object_dst_set_layout.destroy();
                        self.material_dst_set_layout.destroy();
                        self.world_dst_set_layout.destroy();
                        // self.object_matrices_buffers.drain(..).for_each(|b| b.destroy());
                        // self.material_data_buffer.destroy();
                        // self.world_light_buffer.destroy();
                        // self.world_matrices_buffer.destroy();
                        self.setup_cmd_buffer.destroy();
                        // self.draw_cmd_buffers.drain(..).for_each(|cb| cb.destroy());
                        self.render_pass.destroy();
                        self.swapchain.destroy();
                        self.vk_asset_manager.destroy();
                        self.vk_context.destroy();
                }

                Ok(())
        }
}

impl VkRenderer {
        fn recreate_swapchain_maybe(&mut self) -> AnyResult<()> {
                match self.swapchain_outdated_causes {
                        VkSwapchainOutdatedCauseFlags::NONE => return Ok(()),
                        // If resize is the only cause, then check that we actually need to resize
                        VkSwapchainOutdatedCauseFlags::WINDOW_RESIZE => {
                                let wsize = &self.window.inner_size();
                                let ssize = &self.swapchain.extent;

                                if (wsize.width == ssize.width) && (wsize.height == ssize.height) {
                                        self.swapchain_outdated_causes = VkSwapchainOutdatedCauseFlags::NONE;

                                        return Ok(());
                                }
                        },
                        _ => (),
                };

                trace!("Recreating VkSwapchain...");
                scoped_timer!("Recreated VkSwapchain in: ", TimePrefix::Milli);

                unsafe { self.vk_context.device.device_wait_idle()? };

                let srecreation_info = self.swapchain.recreate()?;

                let mut recreate_render_pass: bool = false;
                let mut recreate_pipeline: bool = false;

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

                        self.imgui_renderer
                                .as_mut()
                                .unwrap()
                                .set_render_pass(*self.render_pass)?;

                        recreate_pipeline = true;
                }

                self.swapchain.create_framebuffers(*self.render_pass)?;

                if recreate_pipeline {
                        trace!("Recreating VkGraphicsPipeline...");
                        let new_graphics_pipeline = Self::create_graphics_pipeline(
                                &self.vk_asset_manager.shaders[self.basic_shader_id],
                                &self.vk_context.device,
                                self.swapchain.samples,
                                *self.render_pass,
                                &self.graphics_pipeline_layout,
                        )?;
                        let old_graphics_pipeline =
                                std::mem::replace(&mut self.graphics_pipeline, new_graphics_pipeline);
                        unsafe { old_graphics_pipeline.destroy() };
                }

                self.swapchain_outdated_causes = VkSwapchainOutdatedCauseFlags::NONE;

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

        fn create_world_dst_set_layout(device: &Rc<VkDevice>) -> VkResult<VkDescriptorSetLayout> {
                let bindings = [
                        // WorldMatrices
                        vk::DescriptorSetLayoutBinding {
                                binding: 0,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::VERTEX,
                                p_immutable_samplers: std::ptr::null(),
                        },
                        // WorldLight
                        vk::DescriptorSetLayoutBinding {
                                binding: 1,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                ];

                let create_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

                unsafe { VkDescriptorSetLayout::new(device, &create_info) }
        }

        fn create_material_dst_set_layout(device: &Rc<VkDevice>) -> VkResult<VkDescriptorSetLayout> {
                let bindings = [
                        // Texture
                        vk::DescriptorSetLayoutBinding {
                                binding: 0,
                                descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                        // Texture Sampler
                        vk::DescriptorSetLayoutBinding {
                                binding: 1,
                                descriptor_type: vk::DescriptorType::SAMPLER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                        // Material data
                        vk::DescriptorSetLayoutBinding {
                                binding: 2,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                ];

                let create_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

                unsafe { VkDescriptorSetLayout::new(device, &create_info) }
        }

        fn create_object_dst_set_layout(device: &Rc<VkDevice>) -> VkResult<VkDescriptorSetLayout> {
                let bindings = [
                        // ObjectMatrices
                        vk::DescriptorSetLayoutBinding {
                                binding: 0,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::VERTEX,
                                p_immutable_samplers: std::ptr::null(),
                        },
                ];

                let create_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

                unsafe { VkDescriptorSetLayout::new(device, &create_info) }
        }

        fn create_descriptor_set_layouts(device: &Rc<VkDevice>) -> VkResult<VkDescriptorSetLayout> {
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

                Ok(material_dst_set_layout)
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
        //         vk_shader: VkShader,
        //         shader: Shader,
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
        //                         .module(*vk_shader.vert_module)
        //                         .name(&entry_point)
        //                         .build(),
        //                 vk::PipelineShaderStageCreateInfo::builder()
        //                         .stage(vk::ShaderStageFlags::FRAGMENT)
        //                         .module(*vk_shader.frag_module)
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

        fn should_render(&self) -> bool {
                if self.is_window_minimized() {
                        return false;
                }

                true
        }

        fn is_window_minimized(&self) -> bool {
                let wsize = self.window.inner_size();
                (wsize.width == 0) || (wsize.height == 0)
        }

        unsafe fn begin_frame(&mut self) -> AnyResult<BeginFrameResult> {
                self.recreate_swapchain_maybe()?;
                let frame_data = &mut self.frames_data[self.framei];

                let imagei = {
                        let result = self.swapchain.acquire_next_image(
                                u64::MAX,
                                *frame_data.img_available_semaphore,
                                vk::Fence::null(),
                        );

                        match result {
                                Ok((imagei, suboptimal)) => {
                                        if suboptimal {
                                                self.swapchain_outdated_causes
                                                        .insert(VkSwapchainOutdatedCauseFlags::SUBOPTIMAL);
                                        }

                                        imagei
                                },
                                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                        self.swapchain_outdated_causes
                                                .insert(VkSwapchainOutdatedCauseFlags::OUT_OF_DATE);

                                        return Ok(BeginFrameResult::Skip);
                                },
                                Err(e) => return Err(e.into()),
                        }
                };

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
                        .wait_for_fences(&[*frame_data.draw_cmd_buffer.fence], true, u64::MAX)?;
                self.vk_context
                        .device
                        .reset_fences(&[*frame_data.draw_cmd_buffer.fence])?;

                self.vk_context.device.reset_command_buffer(
                        *frame_data.draw_cmd_buffer,
                        vk::CommandBufferResetFlags::RELEASE_RESOURCES,
                )?;

                let cmd_buffer_binfo =
                        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                self.vk_context
                        .device
                        .begin_command_buffer(*frame_data.draw_cmd_buffer, &cmd_buffer_binfo)?;

                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                        .render_pass(*self.render_pass)
                        .framebuffer(**frame_framebuffer)
                        .render_area(self.swapchain.scissor)
                        .clear_values(&clear_values);

                self.vk_context.device.cmd_begin_render_pass(
                        *frame_data.draw_cmd_buffer,
                        &render_pass_binfo,
                        vk::SubpassContents::INLINE,
                );

                Ok(BeginFrameResult::Draw { imagei })
        }

        unsafe fn end_frame(&mut self, imagei: u32) -> AnyResult<()> {
                let frame_data = &mut self.frames_data[self.framei];

                self.vk_context.device.cmd_end_render_pass(*frame_data.draw_cmd_buffer);
                self.vk_context.device.end_command_buffer(*frame_data.draw_cmd_buffer)?;

                let cmd_buffers = [*frame_data.draw_cmd_buffer];
                let wait_semaphores = [*frame_data.img_available_semaphore];
                let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
                let signal_semaphores = [*frame_data.present_complete_semaphore];

                let submit_info = vk::SubmitInfo::builder()
                        .command_buffers(&cmd_buffers)
                        .wait_semaphores(&wait_semaphores)
                        .wait_dst_stage_mask(&wait_stages)
                        .signal_semaphores(&signal_semaphores)
                        .build();

                self.vk_context.device.queue_submit(
                        self.vk_context.queues.graphics,
                        &[submit_info],
                        *frame_data.draw_cmd_buffer.fence,
                )?;

                match self.swapchain.queue_present(
                        self.vk_context.queues.present,
                        &vk::PresentInfoKHR::builder()
                                .wait_semaphores(&[*frame_data.present_complete_semaphore])
                                .swapchains(&[*self.swapchain])
                                .image_indices(&[imagei]),
                ) {
                        Ok(suboptimal) if suboptimal => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauseFlags::SUBOPTIMAL);
                        },
                        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauseFlags::OUT_OF_DATE);
                        },
                        Err(err) => return Err(err.into()),
                        _ => (),
                };

                self.framei = (self.framei + 1) % self.max_concurrent_frames;

                Ok(())
        }

        fn draw_model(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                object_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                matrices: &WorldMatrices,
                model_id: ModelId,
                model_instance_transform: &Mat4,
                offset: u32,
        ) -> VkResult<()> {
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

                // matrices.normal = glm::inverse_transpose(transform_final);

                //let mut last_material = MaterialID::MAX;
                if let Some(mesh) = model.mesh {
                        Self::draw_mesh(
                                device,
                                draw_cmd_buffer,
                                object_dst_set,
                                pipeline_layout,
                                vk_asset_manager,
                                &asset_manager.meshes()[mesh],
                                offset,
                        );
                }

                for &child in &model.children {
                        Self::draw_model(
                                device,
                                draw_cmd_buffer,
                                object_dst_set,
                                pipeline_layout,
                                asset_manager,
                                vk_asset_manager,
                                matrices,
                                child,
                                model_instance_transform,
                                offset,
                        )?;
                }

                Ok(())
        }

        fn draw_mesh(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                object_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                vk_asset_manager: &VkAssetManager,
                mesh: &Mesh,
                offset: u32,
        ) {
                for primitive in &mesh.primitives {
                        Self::draw_primitive(
                                device,
                                draw_cmd_buffer,
                                object_dst_set,
                                pipeline_layout,
                                vk_asset_manager,
                                primitive,
                                offset,
                        );
                }
        }

        fn draw_primitive(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                object_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                vk_asset_manager: &VkAssetManager,
                primitive: &Primitive,
                offset: u32,
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
                                1,
                                &[material_dst_set, object_dst_set],
                                &[offset],
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

enum BeginFrameResult {
        Draw { imagei: u32 },
        Skip,
}

#[allow(dead_code)]
struct MatricesVPN {
        view: Mat4,
        proj: Mat4,
        normal: Mat4,
}

#[allow(dead_code)]
struct WorldMatrices {
        view: Mat4,
        proj: Mat4,
}

#[allow(dead_code)]
struct WorldLight {
        color: Vec4,
        pos: Vec4,
}

#[allow(dead_code)]
struct WorldResources {
        matrices: WorldMatrices,
        light: WorldLight,
}

#[allow(dead_code)]
struct MaterialData {
        ambient_color: Vec4,
        diffuse_color: Vec4,
}

#[allow(dead_code)]
struct ObjectMatrices {
        model: Mat4,
        mvp: Mat4,
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
