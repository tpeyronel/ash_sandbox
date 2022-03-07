use std::{rc::Rc, slice, time::Instant};

use ash::{prelude::VkResult, vk};

use bevy_ecs::prelude::World;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use winit::{dpi::PhysicalSize, window::Window};

use super::{
        vk_asset_manager::{VkAssetManager, VkCubemap},
        vk_buffer::{VkBuffer, VkDynamicUniformBuffer},
        vk_command_buffer::VkReusableCommandBuffer,
        vk_context::VkContext,
        vk_swapchain::{VkSwapchain, VkSwapchainOutdatedCauseFlags},
        vk_wrapper::{VkDescriptorSetLayout, VkDevice, VkRenderPass, VkSemaphore},
};
use crate::{
        application::InterpGlobalTransform,
        asset_manager::{AssetManager, MeshId},
        components::{ActiveCamera, DirectionalLight, PointLight, ProjectionCamera, Spotlight},
        constants::MAX_OBJECT_MATRICES,
        model_instance_manager::ModelInstance,
        my_glm::*,
        renderer::Renderer,
        util::RefIntoSlice,
};
use crate::{
        constants::{DESIRED_SWAPCHAIN_IMG_COUNT, MAX_CONCURRENT_FRAMES},
        util::DerefIntoSlice,
        AnyResult,
};

pub struct VkRenderer {
        window: Rc<Window>,

        vk_context: VkContext,
        vk_asset_manager: VkAssetManager,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags,

        render_pass: VkRenderPass,

        setup_cmd_buffer: VkReusableCommandBuffer,

        max_concurrent_frames: usize,
        frames_data: Vec<VkFrameData>,

        world_dst_set_layout: VkDescriptorSetLayout,
        object_dst_set_layout: VkDescriptorSetLayout,

        imgui_renderer: Option<imgui_rs_vulkan_renderer::Renderer>,

        creation_instant: Instant,
        framei: usize,
}

impl VkRenderer {
        pub fn new(window: Rc<Window>, imguic: &mut imgui::Context) -> AnyResult<Self> {
                let vk_context = VkContext::new(Rc::clone(&window))?;

                let mut swapchain = VkSwapchain::new(
                        Rc::clone(&window),
                        Rc::clone(&vk_context.instance),
                        Rc::clone(&vk_context.surface),
                        **vk_context.pdevice,
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

                let vk_asset_manager = VkAssetManager::new(
                        Rc::clone(&vk_context.instance),
                        Rc::clone(&vk_context.pdevice),
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.allocator),
                        vk_context.queues.graphics,
                        Rc::clone(&vk_context.cmd_pool),
                        swapchain.samples,
                        *render_pass,
                        *world_dst_set_layout,
                        *object_dst_set_layout,
                        swapchain.img_count as usize,
                )?;
                trace!("Created VkAssetManager");

                let imgui_renderer_options = imgui_rs_vulkan_renderer::Options {
                        in_flight_frames: max_concurrent_frames,
                        enable_depth_test: false,
                        enable_depth_write: false,
                        sample_count: swapchain.samples,
                };

                let imgui_renderer = Some(imgui_rs_vulkan_renderer::Renderer::with_default_allocator(
                        &**vk_context.instance,
                        **vk_context.pdevice,
                        (**vk_context.device).clone(),
                        vk_context.queues.graphics,
                        **vk_context.cmd_pool,
                        *render_pass,
                        imguic,
                        Some(imgui_renderer_options),
                )?);

                Ok(Self {
                        window,

                        vk_context,
                        vk_asset_manager,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags::NONE,

                        render_pass,

                        setup_cmd_buffer,
                        max_concurrent_frames,
                        frames_data,

                        world_dst_set_layout,
                        object_dst_set_layout,

                        imgui_renderer,

                        creation_instant: Instant::now(),

                        framei: 0,
                })
        }
}

impl Renderer for VkRenderer {
        fn draw_world(&mut self, world: &mut World, imgui_draw_data: &imgui::DrawData) -> AnyResult<()> {
                self.vk_asset_manager
                        .process_asset_manager_events(world.get_resource::<AssetManager>().unwrap())?;
                world.get_resource_mut::<AssetManager>().unwrap().clear_events();

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

                let camera = world.get_resource::<ActiveCamera>().unwrap().0;
                let camera_orien = world.get::<InterpGlobalTransform>(camera).unwrap().0.rotation;
                let camera_pos = world.get::<InterpGlobalTransform>(camera).unwrap().0.translation;
                let camera_projection = world.get::<ProjectionCamera>(camera).unwrap();

                let inverted_view_mat = Mat4::from_translation(camera_pos) * Mat4::from_quat(camera_orien);
                let view_mat = inverted_view_mat.inverse();

                let proj_mat = camera_projection.calc_proj_matrix(aspect_ratio);

                let world_matrices = WorldMatrices {
                        view_pos: Vec4::from((camera_pos, 1.0)),
                        view: view_mat,
                        proj: proj_mat,
                        vp: proj_mat * view_mat,
                };

                frame_data.world_matrices_buffer.write(&world_matrices)?;

                let (light_transform, point_light) = world
                        .query::<(&InterpGlobalTransform, &PointLight)>()
                        .iter(&world)
                        .next()
                        .unwrap();

                let point_light = WorldPointLight {
                        pos: Vec4::from((light_transform.0.translation, 1.0)),
                        color: Vec4::from((point_light.color, 1.0)),
                        kc_kl_kq: Vec4::new(point_light.kc, point_light.kl, point_light.kq, 0.0),
                };

                let dir_light = world.query::<&DirectionalLight>().iter(&world).next().unwrap();

                let dir_light = WorldDirectionalLight {
                        direction: Vec4::from((dir_light.direction, 0.0)),
                        color: Vec4::from((dir_light.color, 1.0)),
                };

                let (spotlight_transform, spotlight_component) = world
                        .query::<(&InterpGlobalTransform, &Spotlight)>()
                        .iter(&world)
                        .next()
                        .unwrap();

                let spotlight_dir = spotlight_transform.0.rotation * Vec3::new(0.0, 0.0, -1.0);
                let spotlight = WorldSpotlight {
                        pos: Vec4::from((spotlight_transform.0.translation, 1.0)),
                        dir: Vec4::from((spotlight_dir, spotlight_component.radius_angle.cos())),
                        color: Vec4::from((spotlight_component.color, 1.0)),
                        kc_kl_kq_inner: Vec4::new(
                                spotlight_component.kc,
                                spotlight_component.kl,
                                spotlight_component.kq,
                                spotlight_component.inner_radius_percentage,
                        ),
                };

                frame_data.world_lights_buffer.write(&WorldLights {
                        dir_light,
                        point_light,
                        spotlight,
                })?;

                unsafe {
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

                        let asset_manager = world.remove_resource::<AssetManager>().unwrap();

                        let skybox = &self.vk_asset_manager.cubemaps[asset_manager.default_cubemap];

                        frame_data.update_skybox(&self.vk_context.device, skybox);

                        let graphics_pipeline_layout = *self.vk_asset_manager.graphics_pipeline_layout;
                        self.vk_context.device.cmd_bind_descriptor_sets(
                                *frame_data.draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                graphics_pipeline_layout,
                                0,
                                &[frame_data.world_dst_set],
                                &[],
                        );

                        let mut buffer_transform_idx = 0;

                        let cube_root_model = &asset_manager.models()[asset_manager.get_model_by_name("cube")];
                        let cube_model = cube_root_model.children[0];

                        Self::draw_model_instance(
                                &self.vk_context.device,
                                *frame_data.draw_cmd_buffer,
                                &frame_data.object_matrices_buffer,
                                frame_data.object_dst_set,
                                graphics_pipeline_layout,
                                &asset_manager,
                                &self.vk_asset_manager,
                                self.framei,
                                &WorldMatrices {
                                        view_pos: Vec4::from((camera_pos, 1.0)),
                                        view: Mat4::from_mat3(Mat3::from_mat4(view_mat)),
                                        proj: proj_mat,
                                        vp: proj_mat * Mat4::from_mat3(Mat3::from_mat4(view_mat)),
                                },
                                &ModelInstance { model: cube_model },
                                Mat4::IDENTITY,
                                buffer_transform_idx,
                        )?;

                        buffer_transform_idx += 1;

                        for (minstance, transform) in
                                world.query::<(&ModelInstance, &InterpGlobalTransform)>().iter(world)
                        {
                                Self::draw_model_instance(
                                        &self.vk_context.device,
                                        *frame_data.draw_cmd_buffer,
                                        &frame_data.object_matrices_buffer,
                                        frame_data.object_dst_set,
                                        graphics_pipeline_layout,
                                        &asset_manager,
                                        &self.vk_asset_manager,
                                        self.framei,
                                        &world_matrices,
                                        minstance,
                                        transform.0.to_matrix(),
                                        buffer_transform_idx,
                                )?;

                                buffer_transform_idx += 1;
                        }

                        world.insert_resource(asset_manager);

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
                        drop(self.imgui_renderer.take().unwrap());
                        self.frames_data.clear();
                        self.object_dst_set_layout.destroy();
                        self.world_dst_set_layout.destroy();
                        self.setup_cmd_buffer.destroy();
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
                scoped_timer!("Recreated VkSwapchain in: ", Millis);

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
                        // TODO: recreate pipelines in VkAssetManager
                        // let new_graphics_pipeline = Self::create_graphics_pipeline(
                        //         &self.vk_asset_manager.shaders[self.basic_shader_id],
                        //         &self.vk_context.device,
                        //         self.swapchain.samples,
                        //         *self.render_pass,
                        //         &self.graphics_pipeline_layout,
                        // )?;
                        // let old_graphics_pipeline =
                        // std::mem::replace(&mut self.graphics_pipeline, new_graphics_pipeline);
                        // unsafe { old_graphics_pipeline.destroy() };
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
                                stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                        // WorldDirectionalLight
                        vk::DescriptorSetLayoutBinding {
                                binding: 1,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                        // WorldLight
                        vk::DescriptorSetLayoutBinding {
                                binding: 2,
                                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 1,
                                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                                p_immutable_samplers: std::ptr::null(),
                        },
                        // Skybox
                        vk::DescriptorSetLayoutBinding {
                                binding: 3,
                                descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
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
                let intensity = (((time.sin() + 1.0) / 2.0) * 0.05) + 0.05;

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

                let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
                let submit_info = vk::SubmitInfo::builder()
                        // .command_buffers(&cmd_buffers)
                        .command_buffers(frame_data.draw_cmd_buffer.deref_into_slice())
                        .wait_semaphores(frame_data.img_available_semaphore.deref_into_slice())
                        .wait_dst_stage_mask(&wait_stages)
                        .signal_semaphores(frame_data.present_complete_semaphore.deref_into_slice())
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

        fn draw_model_instance(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                object_matrices_buffer: &VkDynamicUniformBuffer<ObjectMatrices>,
                object_dst_set: vk::DescriptorSet,
                pipeline_layout: vk::PipelineLayout,
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                framei: usize,
                world_matrices: &WorldMatrices,
                minstance: &ModelInstance,
                model_matrix: Mat4,
                buffer_transform_idx: usize,
        ) -> VkResult<()> {
                //let mut last_material = MaterialID::MAX;

                unsafe {
                        let model = model_matrix;
                        let mvp = world_matrices.vp * model;
                        let normal = model.inverse().transpose();

                        let object_matrices = ObjectMatrices { model, mvp, normal };
                        let object_matrices_offset =
                                object_matrices_buffer.write(&object_matrices, buffer_transform_idx)?;

                        device.cmd_bind_descriptor_sets(
                                draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                pipeline_layout,
                                2,
                                &[object_dst_set],
                                &[object_matrices_offset as u32],
                        )
                };

                let model = &asset_manager.models()[minstance.model];

                for &mesh_id in &model.meshes {
                        Self::draw_mesh_instance(
                                device,
                                draw_cmd_buffer,
                                pipeline_layout,
                                asset_manager,
                                vk_asset_manager,
                                framei,
                                mesh_id,
                        )?;
                }

                Ok(())
        }

        fn draw_mesh_instance(
                device: &VkDevice,
                draw_cmd_buffer: vk::CommandBuffer,
                pipeline_layout: vk::PipelineLayout,
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                framei: usize,
                mesh_id: MeshId,
        ) -> VkResult<()> {
                let mesh = &asset_manager.meshes()[mesh_id];
                let material = &asset_manager.materials()[mesh.material];
                let pipeline = *vk_asset_manager.pipelines[material.shader];

                let vk_mesh = &vk_asset_manager.meshes[mesh_id];
                let vk_material = &vk_asset_manager.materials[mesh.material];

                /* if mesh.material != last_material {
                        last_material = mesh.material;
                } */

                unsafe {
                        device.cmd_bind_pipeline(draw_cmd_buffer, vk::PipelineBindPoint::GRAPHICS, pipeline);

                        // TODO: update all materials beforehand, to avoid updating the same material if its shared by multiple meshes.
                        let offset = vk_material.material_data_buffer.write(
                                &MaterialData {
                                        ambient_color: material.base_color_factor,
                                        diffuse_color: material.base_color_factor,
                                        specular_color: material.base_color_factor,
                                        shininess_and_ambient_strength: Vec2::new(
                                                material.shininess,
                                                material.ambient_strength,
                                        ),
                                        specular_strength_and_diffuse_strength: Vec2::new(
                                                material.specular_strength,
                                                material.diffuse_strength,
                                        ),
                                },
                                framei,
                        )?;

                        device.cmd_bind_descriptor_sets(
                                draw_cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                pipeline_layout,
                                1,
                                &[vk_material.dst_set],
                                &[offset as u32],
                        );
                        device.cmd_bind_vertex_buffers(
                                draw_cmd_buffer,
                                0,
                                &[*vk_mesh.positions, *vk_mesh.normals, *vk_mesh.tex_coords],
                                &[0, 0, 0],
                        );
                        device.cmd_bind_index_buffer(
                                draw_cmd_buffer,
                                *vk_mesh.indices.buffer,
                                0,
                                vk_mesh.indices.index_type,
                        );

                        device.cmd_draw_indexed(draw_cmd_buffer, vk_mesh.indices.index_count, 1, 0, 0, 0);
                }

                Ok(())
        }
}

struct VkFrameData {
        // Signaled when a swapchain image has become available for presentation. vkAcquireImage may return an image that is not immediately available.
        img_available_semaphore: VkSemaphore,
        // Signaled when a swapchain image presentation has completed
        present_complete_semaphore: VkSemaphore,
        // Command buffer used for submitting draw operations of one frame.
        draw_cmd_buffer: VkReusableCommandBuffer,

        world_dst_set: vk::DescriptorSet,
        object_dst_set: vk::DescriptorSet,

        world_matrices_buffer: VkBuffer,
        world_lights_buffer: VkBuffer,
        material_data_buffer: VkBuffer,
        object_matrices_buffer: VkDynamicUniformBuffer<ObjectMatrices>,
}

impl VkFrameData {
        fn new(
                vk_context: &VkContext,
                world_dst_set_layout: vk::DescriptorSetLayout,
                object_dst_set_layout: vk::DescriptorSetLayout,
        ) -> AnyResult<Self> {
                let semaphore_cinfo = vk::SemaphoreCreateInfo::builder().build();
                let img_available_semaphore = unsafe { VkSemaphore::new(&vk_context.device, &semaphore_cinfo)? };
                let present_complete_semaphore = unsafe { VkSemaphore::new(&vk_context.device, &semaphore_cinfo)? };

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

                let world_lights_buffer_size = std::mem::size_of::<WorldLights>() as vk::DeviceSize;
                let world_lights_buffer = VkBuffer::new_uniform_buffer(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        world_lights_buffer_size,
                )?;

                let material_data_buffer_size = std::mem::size_of::<MaterialData>() as vk::DeviceSize;
                let material_data_buffer = VkBuffer::new_uniform_buffer(
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        material_data_buffer_size,
                )?;

                let object_matrices_buffer = VkDynamicUniformBuffer::new(
                        &vk_context.pdevice,
                        &vk_context.device,
                        Rc::clone(&vk_context.allocator),
                        MAX_OBJECT_MATRICES,
                )?;

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

                let world_lights_buffer_info = vk::DescriptorBufferInfo {
                        buffer: *world_lights_buffer,
                        offset: 0,
                        range: world_lights_buffer_size,
                };

                let world_lights_dst_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .dst_set(world_dst_set)
                        .dst_binding(1)
                        .dst_array_element(0)
                        .buffer_info(std::slice::from_ref(&world_lights_buffer_info))
                        .build();

                let object_matrices_buffer_info = vk::DescriptorBufferInfo {
                        buffer: *object_matrices_buffer,
                        offset: 0,
                        range: object_matrices_buffer.element_padded_size() as vk::DeviceSize,
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
                        world_lights_dst_write,
                        object_matrices_dst_write,
                ];
                unsafe { vk_context.device.update_descriptor_sets(&writes, &[]) };

                Ok(Self {
                        img_available_semaphore,
                        present_complete_semaphore,
                        draw_cmd_buffer,
                        world_dst_set,
                        object_dst_set,
                        world_matrices_buffer,
                        world_lights_buffer,
                        material_data_buffer,
                        object_matrices_buffer,
                })
        }

        fn update_skybox(&self, device: &VkDevice, skybox: &VkCubemap) {
                let world_skybox_image_info = vk::DescriptorImageInfo {
                        sampler: *skybox.sampler,
                        image_view: *skybox.image_view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                };

                let world_skybox_write = vk::WriteDescriptorSet::builder()
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .dst_set(self.world_dst_set)
                        .dst_binding(3)
                        .dst_array_element(0)
                        .image_info(world_skybox_image_info.ref_into_slice())
                        .build();

                unsafe { device.update_descriptor_sets(&[world_skybox_write], &[]) };
        }
}

impl Drop for VkFrameData {
        fn drop(&mut self) {
                unsafe {
                        self.present_complete_semaphore.destroy();
                        self.img_available_semaphore.destroy();
                        self.object_matrices_buffer.destroy();
                        self.material_data_buffer.destroy();
                        self.world_lights_buffer.destroy();
                        self.world_matrices_buffer.destroy();
                        self.draw_cmd_buffer.destroy();
                }
        }
}

enum BeginFrameResult {
        Draw { imagei: u32 },
        Skip,
}

#[allow(dead_code)]
struct WorldMatrices {
        view_pos: Vec4,
        view: Mat4,
        proj: Mat4,
        vp: Mat4,
}

#[allow(dead_code)]
struct WorldDirectionalLight {
        direction: Vec4,
        color: Vec4,
}

#[allow(dead_code)]
struct WorldPointLight {
        pos: Vec4,
        color: Vec4,
        kc_kl_kq: Vec4,
}

#[allow(dead_code)]
struct WorldSpotlight {
        pos: Vec4,
        dir: Vec4, // xyz=direction w=angle
        color: Vec4,
        kc_kl_kq_inner: Vec4, // w=inner radius percentage
}

#[allow(dead_code)]
struct WorldLights {
        dir_light: WorldDirectionalLight,
        point_light: WorldPointLight,
        spotlight: WorldSpotlight,
}

#[allow(dead_code)]
pub struct MaterialData {
        ambient_color: Vec4,
        diffuse_color: Vec4,
        specular_color: Vec4,
        shininess_and_ambient_strength: Vec2,
        specular_strength_and_diffuse_strength: Vec2,
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
